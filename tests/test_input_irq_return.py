"""Execute production IRQ save/restore and return-frame policy on the host.

Only privileged IRET is replaced by a capture trampoline. These tests do not
claim to execute a hardware interrupt; the desktop QEMU gate covers that path.
"""
import pathlib
import re
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "rust/kernel_core/src/arch/x86_64/keyboard_entry.S"
USER_ENTRY = ROOT / "rust/kernel_core/src/arch/x86_64/user_entry.S"
REGISTERS = "rax rbx rcx rdx rsi rdi rbp r8 r9 r10 r11 r12 r13 r14 r15".split()


def fixture(source):
    source = source.replace("{mouse_irq}", "1")
    source, count = re.subn(r"(?m)^    iretq$", "    jmp test_irq_return", source)
    if count != 2:
        raise AssertionError("unexpected production IRET layout")
    # Link the actual production helpers and zero-initialized ownership globals.
    # This IRQ fixture has no active user FPU lease; do not replace the helpers
    # with empty test functions to make the assembly link.
    user_entry = USER_ENTRY.read_text()
    helpers = user_entry[user_entry.index(".bss"):user_entry.index(".global x86_64_enter_user")]
    asm = [helpers, source, ".text"]
    for name, target in (("keyboard", "x86_64_keyboard_interrupt"),
                         ("mouse", "x86_64_mouse_interrupt")):
        asm += [f".global run_{name}", f"run_{name}:",
                "pushq %rbx", "pushq %rbp", "pushq %r12", "pushq %r13",
                "pushq %r14", "pushq %r15", "subq $8, %rsp",
                "movq %rsp, host_sp(%rip)", "pushq frame_flags(%rip)",
                "pushq frame_cs(%rip)", "pushq $0x12345678", "std"]
        asm += [f"movq ${0x101 + i}, %{reg}" for i, reg in enumerate(REGISTERS)]
        asm += [f"jmp {target}"]
    for name in ("keyboard", "mouse"):
        asm += [f".global x86_64_{name}_irq_dispatch",
                f"x86_64_{name}_irq_dispatch:", "pushfq", "popq %rax",
                "movq %rax, dispatcher_flags(%rip)",
                "incq dispatch_count(%rip)", "movq %rsp, dispatcher_sp(%rip)"]
        asm += [f"movq $0xBAD, %{reg}" for reg in REGISTERS]
        asm += ["ret"]
    asm += ["test_irq_return:"]
    asm += [f"movq %{reg}, result+{i * 8}(%rip)" for i, reg in enumerate(REGISTERS)]
    asm += ["cld", "movq 0(%rsp), %rax", "movq %rax, result+120(%rip)",
            "movq 8(%rsp), %rax", "movq %rax, result+128(%rip)",
            "movq 16(%rsp), %rax", "movq %rax, result+136(%rip)",
            "movq host_sp(%rip), %rsp", "addq $8, %rsp", "popq %r15",
            "popq %r14", "popq %r13", "popq %r12", "popq %rbp", "popq %rbx", "ret",
            '.section .note.GNU-stack,"",@progbits']
    return "\n".join(asm) + "\n"


C_SOURCE = r'''
#include <stdint.h>
#include <stdio.h>
uint64_t host_sp, frame_flags, frame_cs, dispatcher_flags, dispatcher_sp;
uint64_t dispatch_count, result[18];
extern void run_keyboard(void), run_mouse(void);
int main(void) {
    void (*entries[])(void) = {run_keyboard, run_mouse};
    const uint64_t flags[] = {2, 0x10002, 0x10643, 0x643, 0x200A47};
    for (unsigned entry = 0; entry < 2; ++entry)
        for (unsigned user = 0; user < 2; ++user)
            for (unsigned f = 0; f < sizeof(flags)/sizeof(flags[0]); ++f) {
                frame_cs = user ? 0x2b : 0x08;
                frame_flags = flags[f];
                dispatch_count = 0;
                entries[entry]();
                for (unsigned reg = 0; reg < 15; ++reg)
                    if (result[reg] != 0x101 + reg) {
                        fprintf(stderr, "GPR %u changed\n", reg); return 1;
                    }
                uint64_t expected = user ? frame_flags & ~(1ULL << 16) : frame_flags;
                if (result[15] != 0x12345678 || result[16] != frame_cs || result[17] != expected) {
                    fprintf(stderr, "return frame changed: entry=%u user=%u flags=%llx expected=%llx\n",
                        entry, user, (unsigned long long)result[17], (unsigned long long)expected);
                    return 2;
                }
                if (dispatch_count != 1 || (dispatcher_flags & (1 << 10)) || (dispatcher_sp & 15) != 8) {
                    fprintf(stderr, "dispatcher count/DF/stack alignment invalid\n"); return 3;
                }
            }
    return 0;
}
'''


class InputIrqReturnTests(unittest.TestCase):
    def execute(self, source):
        with tempfile.TemporaryDirectory(prefix="indianos-irq-return-") as directory:
            directory = pathlib.Path(directory)
            (directory / "irq.S").write_text(fixture(source))
            (directory / "oracle.c").write_text(C_SOURCE)
            subprocess.run(["gcc", "-Wall", "-Wextra", "-Werror", "-no-pie",
                            str(directory / "irq.S"), str(directory / "oracle.c"),
                            "-o", str(directory / "oracle")], check=True, capture_output=True, text=True)
            return subprocess.run([str(directory / "oracle")], capture_output=True, text=True, timeout=5)

    def test_production_keyboard_and_mouse_restore_contract(self):
        result = self.execute(SOURCE.read_text())
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_oracle_rejects_missing_user_rf_normalization(self):
        source = SOURCE.read_text().replace("btrq $16, 16(%rsp)", "nop")
        result = self.execute(source)
        self.assertEqual(result.returncode, 2, result.stderr)

    def test_oracle_rejects_supervisor_frame_normalization(self):
        source = SOURCE.read_text().replace("    jz 1f", "    nop")
        result = self.execute(source)
        self.assertEqual(result.returncode, 2, result.stderr)

    def test_oracle_rejects_wrong_register_restore(self):
        source = SOURCE.read_text().replace("    popq %rax", "    popq %rcx")
        result = self.execute(source)
        self.assertEqual(result.returncode, 1, result.stderr)


if __name__ == "__main__":
    unittest.main()
