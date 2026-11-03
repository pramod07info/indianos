#!/usr/bin/env python3
"""Fail-closed bounded retry for transient GitHub commit-status API failures.

This helper publishes no status itself. Callers must construct their exact
commit SHA, state, context and description before passing a POST Request.
All attempts reuse the same immutable request. 4xx auth/permission errors
are NOT retried. Repeated 429/5xx or network errors still fail the workflow.
"""
import re
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

RETRY_CODES = frozenset((429, 500, 502, 503, 504))
MAX_ATTEMPTS = 5
STATUS_PATH = re.compile(r"/repos/[^/]+/[^/]+/statuses/[a-f0-9]{40}")


def post_status_with_retry(request, timeout=20, *, opener=urlopen,
                           sleeper=time.sleep):
    """Open an exact-SHA status POST, retrying *only* transient failures.

    Returns the still-open response, so normal 'with ... as response' works.
    Never changes payload, state, SHA or target URL between attempts.
    """
    if not isinstance(request, Request) or request.get_method() != "POST":
        raise ValueError("expected GitHub commit-status POST Request")
    address = urlsplit(request.full_url)
    if (address.scheme != "https" or address.netloc != "api.github.com"
            or not STATUS_PATH.fullmatch(address.path)
            or address.query or address.fragment or not request.data):
        raise ValueError("expected exact GitHub commit-status URL and body")
    for attempt in range(MAX_ATTEMPTS):
        try:
            return opener(request, timeout=timeout)
        except HTTPError as error:
            if error.code not in RETRY_CODES or attempt + 1 == MAX_ATTEMPTS:
                raise
            code = str(error.code)
        except URLError:
            if attempt + 1 == MAX_ATTEMPTS:
                raise
            code = "network"
        delay = 2 ** attempt
        print(f"Transient GitHub status response ({code}); retry "
              f"{attempt + 2}/{MAX_ATTEMPTS} in {delay}s", file=sys.stderr)
        sleeper(delay)
    raise AssertionError("unreachable: retry loop exhausted")
