/* Host-only test shim for no_std Rust staticlib linked by a C test main.
 * The distributed bare-metal Rust target never links this file.
 *
 * Prebuilt Linux libcore may reference its unwinding personality metadata
 * even when this crate is compiled with panic=abort. These C ABI tests
 * explicitly forbid unwinding; fail closed if it is ever attempted.
 */
__attribute__((noreturn)) void rust_eh_personality(void) {
    __builtin_trap();
}
