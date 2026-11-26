"""Exercise production recoverable-exception register/frame assembly on host.

CLI and IRET alone are replaced: this is an ABI oracle, not a hardware fault.
The separate QEMU fixture supplies the actual absent-page faults.
"""
import pathlib
import subprocess
import tempfile
import unittest

from test_input_irq_return import C_SOURCE, REGISTERS, ROOT, USER_ENTRY

SOURCE = ROOT / "rust/kernel_core/src/arch/x86_64/interrupt_entry.S"


def fixture(source):
    common = source[source.index(".type x86_64_isr_common,"):
                    source.index(".global x86_64_isr_0")]
    if common.count("    cli") != 1 or common.count("    iretq") != 1:
        raise AssertionError("unexpected recoverable exception boundary")
    common = common.replace("    cli", "    nop").replace("    iretq", "    jmp test_fault_return")
    entries = USER_ENTRY.read_text()
    helpers = entries[entries.index(".bss"):entries.index(".global x86_64_enter_user")]
    asm = [helpers, ".text", common]
    for name in ("keyboard", "mouse"):
        asm += [f".global run_{name}", f"run_{name}:",
                "pushq %rbx", "pushq %rbp", "pushq %r12", "pushq %r13",
                "pushq %r14", "pushq %r15", "subq $8, %rsp",
                "movq %rsp, host_sp(%rip)", "pushq frame_flags(%rip)",
                "pushq frame_cs(%rip)", "pushq $0x12345678",
                "pushq $6", "pushq $14", "std"]
        asm += [f"movq ${0x101+i}, %{reg}" for i, reg in enumerate(REGISTERS)]
        asm += ["jmp x86_64_isr_common"]
    asm += [".global x86_64_exception_dispatch", "x86_64_exception_dispatch:",
            "pushfq", "popq %rax", "movq %rax, dispatcher_flags(%rip)",
            "movq %rsp, dispatcher_sp(%rip)", "incq dispatch_count(%rip)",
            "cmpq $14, 0(%rdi)", "jne 3f", "cmpq $6, 8(%rdi)", "jne 3f",
            "cmpq $0x12345678, 16(%rdi)", "jne 3f", "cmpq $0x2b, 24(%rdi)",
            "je 4f", "3: movq $1, context_bad(%rip)", "4:"]
    # Rust preserves callee-saved registers, including the common stub's R12
    # stack cursor. Deliberately clobber every caller-saved GPR instead.
    asm += [f"movq $0xBAD, %{reg}" for reg in REGISTERS
            if reg not in ("rbx", "rbp", "r12", "r13", "r14", "r15")]
    asm += ["ret", "test_fault_return:"]
    asm += [f"movq %{reg}, result+{i*8}(%rip)" for i, reg in enumerate(REGISTERS)]
    asm += ["cld", "movq 0(%rsp), %rax", "movq %rax, result+120(%rip)",
            "movq 8(%rsp), %rax", "movq %rax, result+128(%rip)",
            "movq 16(%rsp), %rax", "movq %rax, result+136(%rip)",
            "movq host_sp(%rip), %rsp", "addq $8, %rsp", "popq %r15",
            "popq %r14", "popq %r13", "popq %r12", "popq %rbp", "popq %rbx", "ret",
            '.section .note.GNU-stack,"",@progbits']
    return "\n".join(asm) + "\n"


class DemandFaultReturnTests(unittest.TestCase):
    def execute(self, source):
        oracle = C_SOURCE.replace("unsigned user = 0", "unsigned user = 1")
        oracle = oracle.replace("uint64_t dispatch_count, result[18];",
                                "uint64_t dispatch_count, result[18], context_bad;")
        oracle = oracle.replace("if (dispatch_count != 1", "if (context_bad || dispatch_count != 1")
        with tempfile.TemporaryDirectory(prefix="indianos-demand-return-") as directory:
            directory = pathlib.Path(directory)
            (directory / "entry.S").write_text(fixture(source))
            (directory / "oracle.c").write_text(oracle)
            subprocess.run(["gcc", "-Wall", "-Wextra", "-Werror", "-no-pie",
                            str(directory / "entry.S"), str(directory / "oracle.c"),
                            "-o", str(directory / "oracle")], check=True, capture_output=True, text=True)
            return subprocess.run([str(directory / "oracle")], capture_output=True, text=True, timeout=5)

    def test_production_context_gprs_alignment_and_return_flags(self):
        result = self.execute(SOURCE.read_text())
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_wrong_context_pointer_is_rejected(self):
        result = self.execute(SOURCE.read_text().replace("leaq 120(%rsp)", "leaq 112(%rsp)"))
        self.assertEqual(result.returncode, 3, result.stderr)

    def test_wrong_register_restore_is_rejected(self):
        result = self.execute(SOURCE.read_text().replace("    popq %rax", "    popq %rcx"))
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_missing_rf_normalization_is_rejected(self):
        result = self.execute(SOURCE.read_text().replace("btrq $16, 32(%rsp)", "nop"))
        self.assertEqual(result.returncode, 2, result.stderr)


if __name__ == "__main__":
    unittest.main()
