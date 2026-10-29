# M1 Terminal literal text fidelity

The mounted persistent Terminal now preserves the exact literal argument bytes
entered through the existing bounded ASCII input editor. Previously the command
dispatcher trimmed the entire entered line, silently discarding trailing spaces
before writes, appends, replacements and text searches.

Leading spaces before a command remain ignorable. Trailing spaces are still
ignored for command-only, path-only, numeric and source/destination path syntax.
For `write`, `append`, `appendln`, `pwrite`, `replace`, `find` and `grep`, bytes
after the existing argument separator are literal text, including their final
spaces. The separator itself is not payload: `write /note   ` writes two spaces.
An absent or zero-byte payload is still invalid. Literal-space-only replacements
and search needles now work without quotes or escape interpretation.

This is the existing 192-byte command grammar, not a POSIX shell. Input remains
printable ASCII, with no Unicode keyboard input, quoting, variable expansion or
control-sequence execution. Files may still contain UTF-8 when populated through
other authenticated APIs; this change does not add Unicode keyboard decoding.

The authorization, namespace protection, response-buffer preflight and COW
publication paths are unchanged. Failed or unauthorized mutations retain the
previous durable data and do not emit a successful response.

Regression tests type complete lines through `InputLine`, submit them to the real
Terminal dispatcher and remount its block-backed filesystem after each command.
They cover write, space-only append, offset writes, append-with-newline, maximum
edited line length, exact search/replacement, syntax compatibility, insufficient
response buffers, stale identities and device write failures.

Run the focused regressions with:

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --features test-r031-vfs m1_terminal_
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --features test-r031-vfs
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml \
  --no-default-features --target x86_64-unknown-none --features test-r031-vfs
```
