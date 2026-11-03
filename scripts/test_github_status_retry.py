#!/usr/bin/env python3
"""Negative and positive regressions for fail-closed GitHub status retries."""
import io
import unittest
from unittest.mock import Mock
from urllib.error import HTTPError, URLError
from urllib.request import Request

from tools.github_status_retry import MAX_ATTEMPTS, post_status_with_retry

URL = "https://api.github.com/repos/pramod07info/indianos/statuses/" + "a" * 40


def request(url=URL, method="POST", body=b'{"state":"success"}'):
    return Request(url, data=body, method=method)


def failure(code):
    return HTTPError(URL, code, "mock response", {}, io.BytesIO())


class GitHubCommitStatusRetryTests(unittest.TestCase):
    def test_one_transient_503_retries_identical_sha_payload_and_succeeds(self):
        r = request()
        response = object()
        opener = Mock(side_effect=[failure(503), response])
        sleeper = Mock()
        self.assertIs(post_status_with_retry(r, opener=opener, sleeper=sleeper), response)
        self.assertEqual(opener.call_count, 2)
        self.assertEqual([c.args[0] for c in opener.call_args_list], [r, r])
        sleeper.assert_called_once_with(1)

    def test_transient_429_502_503_504_500_all_retry(self):
        for code in [429, 500, 502, 503, 504]:
            opener = Mock(side_effect=[failure(code), object()])
            self.assertIsNotNone(post_status_with_retry(
                request(), opener=opener, sleeper=lambda _: None))
            self.assertEqual(opener.call_count, 2)

    def test_permanent_errors_are_never_retried(self):
        for code in [400, 401, 403, 404, 422]:
            opener = Mock(side_effect=[failure(code)])
            with self.assertRaises(HTTPError):
                post_status_with_retry(request(), opener=opener,
                                       sleeper=lambda _: None)
            self.assertEqual(opener.call_count, 1)

    def test_exhausted_transient_response_still_fails_closed(self):
        opener = Mock(side_effect=[failure(503) for _ in range(MAX_ATTEMPTS)])
        sleeper = Mock()
        with self.assertRaises(HTTPError):
            post_status_with_retry(request(), opener=opener, sleeper=sleeper)
        self.assertEqual(opener.call_count, MAX_ATTEMPTS)
        self.assertEqual([call.args[0] for call in sleeper.call_args_list],
                         [1, 2, 4, 8])

    def test_network_failures_retry_but_do_not_disappear(self):
        opener = Mock(side_effect=[URLError("temporary network issue"),
                                   URLError("temporary network issue"),
                                   object()])
        self.assertIsNotNone(post_status_with_retry(
            request(), opener=opener, sleeper=lambda _: None))
        self.assertEqual(opener.call_count, 3)
        always = Mock(side_effect=[URLError("offline")] * MAX_ATTEMPTS)
        with self.assertRaises(URLError):
            post_status_with_retry(request(), opener=always,
                                   sleeper=lambda _: None)

    def test_wrong_target_or_method_never_calls_network(self):
        bad = [
            request(url=URL.replace("/statuses/", "/commits/")),
            request(url=URL.replace("api.github.com", "evil.example")),
            request(url=URL.replace("a" * 40, "deadbeef")),
            request(url=URL + "?ignore=1"),
            request(url=URL.replace("https:", "http:")),
            request(method="GET", body=None),
            request(body=b""),
        ]
        for candidate in bad:
            opener = Mock()
            with self.assertRaises(ValueError):
                post_status_with_retry(candidate, opener=opener)
            opener.assert_not_called()


if __name__ == "__main__":
    unittest.main()
