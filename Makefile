# IndianOS Milestone 001 — freestanding x86-64 kernel, Limine, QEMU/OVMF.
# Host OS: Linux/WSL. No physical disks are accessed by these targets.
.DEFAULT_GOAL := kernel
.DELETE_ON_ERROR:

CC ?= clang
# GNU make's built-in CC=cc is overridden for a consistent bare-metal target.
CC := clang
QEMU ?= qemu-system-x86_64
OVMF_CODE ?= $(firstword $(wildcard /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_CODE.fd))

BUILD := build
# Rust-native process/scheduler/fault policy is the verified default.
# RUST_CORE=0 retains the original C implementation as an emergency fallback.
RUST_CORE ?= 1
# R013 opt-in scripted userland boot. No keyboard or arbitrary exec yet.
R013_SHELL_TEST ?= 0
R014_INTERACTIVE_TEST ?= 0
R014_IRQ_TEST ?= 0
R015_ATA_TEST ?= 0
# Isolated QEMU-only, explicitly disposable persistent-drive test.
R027_DURABLE_TEST ?= 0
# Explicit single-core QEMU virtio-net network smoke; disabled ordinarily.
R028_NETWORK_TEST ?= 0
# R029 visible QEMU framebuffer compositor and PS/2 mouse/keyboard.
R029_DESKTOP_TEST ?= 0
# QEMU ring-3 surface command test; no user framebuffer mappings.
R029_USERSPACE_TEST ?= 0
# Build native ring-3 signed package SDK fixtures; off by default.
R030_PACKAGE_V1 ?= 0
R030_PACKAGE_V2 ?= 0
ifeq ($(R030_PACKAGE_V1),1)
USERMODE_SELFTEST := 1
endif
ifeq ($(R030_PACKAGE_V2),1)
USERMODE_SELFTEST := 1
endif
ifeq ($(R029_USERSPACE_TEST),1)
USERMODE_SELFTEST := 1
endif
R025_IPC_TEST ?= 0
R025_COMPLETE_TEST ?= 0
R026_INTERACTIVE_TEST ?= 0
ifeq ($(R026_INTERACTIVE_TEST),1)
R014_INTERACTIVE_TEST := 1
SCHEDULER_SELFTEST := 1
endif
ifeq ($(R025_COMPLETE_TEST),1)
SCHEDULER_SELFTEST := 1
R025_IPC_TEST := 1
endif
ifeq ($(R025_IPC_TEST),1)
SCHEDULER_SELFTEST := 1
endif
ifeq ($(R014_IRQ_TEST),1)
R014_INTERACTIVE_TEST := 1
endif
# Rust owns the normal kernel link, including Limine handoff, framebuffer,
# descriptor tables, timer, kmain, user ELF fixtures and image tools.
# RUST_CORE=0 selects the complete C/assembly reference configuration.
# Explicit RUST_*=0 overrides retain individual rollback paths.
RUST_VMM ?= $(RUST_CORE)
RUST_PMM ?= $(RUST_CORE)
RUST_USER_IMAGE ?= $(RUST_CORE)
RUST_PAGING ?= $(RUST_CORE)
RUST_PAGING_CPU ?= $(RUST_CORE)
RUST_USER_RUNTIME ?= $(RUST_CORE)
# R011: inline x86 trap/IRET assembly now belongs to the Rust static archive.
RUST_CPU_ENTRY ?= $(RUST_CORE)
RUST_RUNTIME ?= $(RUST_CORE)
RUST_BOOT_STAGING ?= $(RUST_CORE)
RUST_BOOT ?= $(RUST_CORE)
RUST_BOOT_PROTOCOL ?= $(RUST_CORE)
RUST_INTERRUPTS ?= $(RUST_CORE)
RUST_TIMER ?= $(RUST_CORE)
RUST_MAIN ?= $(RUST_CORE)
ifeq ($(RUST_MAIN),1)
ifneq ($(RUST_CORE),1)
$(error RUST_MAIN requires RUST_CORE=1)
endif
endif
ifeq ($(RUST_CPU_ENTRY),1)
ifneq ($(RUST_CORE),1)
$(error RUST_CPU_ENTRY requires RUST_CORE=1)
endif
endif
ifeq ($(RUST_USER_RUNTIME),1)
ifneq ($(RUST_CORE),1)
$(error RUST_USER_RUNTIME requires RUST_CORE=1)
endif
ifneq ($(RUST_PAGING_CPU),1)
$(error RUST_USER_RUNTIME requires RUST_PAGING_CPU=1)
endif
endif
ifeq ($(RUST_PAGING_CPU),1)
ifneq ($(RUST_PAGING),1)
$(error RUST_PAGING_CPU requires RUST_PAGING=1)
endif
endif
ifeq ($(RUST_BOOT_PROTOCOL),1)
ifneq ($(RUST_CORE),1)
$(error RUST_BOOT_PROTOCOL requires RUST_CORE=1)
endif
ifneq ($(RUST_BOOT),1)
$(error RUST_BOOT_PROTOCOL requires RUST_BOOT=1)
endif
endif
# Rust native ET_EXEC applications and deterministic host tools are standard.
RUST_USERLAND ?= $(RUST_CORE)
RUST_TOOLS ?= $(RUST_CORE)
RUST_TOOLS_MANIFEST := tools/rust/Cargo.toml
RUST_TOOLS_BIN := $(BUILD)/rust-tools-target/release/indianos-tools
ifeq ($(RUST_TOOLS),1)
SMOKE := $(RUST_TOOLS_BIN) smoke
SMOKE_DEPS := $(RUST_TOOLS_BIN)
else
SMOKE := python3 tools/smoke_boot.py
SMOKE_DEPS :=
endif
ifeq ($(RUST_BOOT_STAGING),1)
ifneq ($(RUST_USER_IMAGE),1)
$(error RUST_BOOT_STAGING=1 requires RUST_USER_IMAGE=1)
endif
ifneq ($(RUST_PMM),1)
$(error RUST_BOOT_STAGING=1 requires RUST_PMM=1)
endif
endif
ifeq ($(RUST_USER_IMAGE),1)
ifneq ($(RUST_VMM),1)
$(error RUST_USER_IMAGE=1 requires RUST_VMM=1)
endif
endif
CARGO ?= cargo
RUST_MANIFEST := rust/kernel_core/Cargo.toml
RUST_SOURCE := $(wildcard rust/kernel_core/src/*.rs)
RUST_LIB := $(BUILD)/libindianos_kernel_core.a
RUST_FEATURES := $(if $(filter 1,$(PREEMPT_SELFTEST)),--features preempt,) $(if $(filter 1,$(RUST_VMM)),--features vmm,) $(if $(filter 1,$(RUST_PMM)),--features pmm,) $(if $(filter 1,$(RUST_USER_IMAGE)),--features user-image,) $(if $(filter 1,$(RUST_PAGING)),--features paging,) $(if $(filter 1,$(RUST_PAGING_CPU)),--features paging-cpu,) $(if $(filter 1,$(RUST_USER_RUNTIME)),--features user-runtime,) $(if $(filter 1,$(RUST_CPU_ENTRY)),--features cpu-entry,) $(if $(filter 1,$(RUST_RUNTIME)),--features runtime,) $(if $(filter 1,$(RUST_BOOT_STAGING)),--features boot-staging,) $(if $(filter 1,$(RUST_BOOT)),--features boot,) $(if $(filter 1,$(RUST_BOOT_PROTOCOL)),--features limine-requests,) $(if $(filter 1,$(RUST_INTERRUPTS)),--features interrupts,) $(if $(filter 1,$(RUST_TIMER)),--features timer,) $(if $(filter 1,$(RUST_MAIN)),--features kernel-main,)
ifeq ($(RUST_MAIN),1)
RUST_FEATURES += $(if $(filter 1,$(EXCEPTION_SELFTEST)),--features test-ud2,) \
 $(if $(filter 1,$(PAGEFAULT_SELFTEST)),--features test-pagefault,) \
 $(if $(filter 1,$(PAGING_SWITCH_SELFTEST)),--features test-shadow,) \
 $(if $(filter 1,$(USERMODE_SELFTEST)),--features test-ring3,) \
 $(if $(filter 1,$(PROCESS_SELFTEST)),--features test-processes,) \
 $(if $(filter 1,$(SCHEDULER_SELFTEST)),--features test-scheduler,) $(if $(filter 1,$(R013_SHELL_TEST)),--features test-shell,) $(if $(filter 1,$(R014_INTERACTIVE_TEST)),--features test-interactive,) $(if $(filter 1,$(R014_IRQ_TEST)),--features test-irq-keyboard,) $(if $(filter 1,$(R015_ATA_TEST)),--features test-ata-disk,) $(if $(filter 1,$(R027_DURABLE_TEST)),--features test-r027-durable,) $(if $(filter 1,$(R028_NETWORK_TEST)),--features test-r028-network,) $(if $(filter 1,$(R029_DESKTOP_TEST)),--features test-r029-desktop,) $(if $(filter 1,$(R029_USERSPACE_TEST)),--features test-r029-userspace,) $(if $(filter 1,$(R026_INTERACTIVE_TEST)),--features test-r026,) $(if $(filter 1,$(R025_COMPLETE_TEST)),--features test-r025,) $(if $(filter 1,$(R025_IPC_TEST)),--features test-ipc,)
ifeq ($(USERFAULT_SELFTEST),1)
RUST_FEATURES += $(if $(filter guard,$(USERFAULT_KIND)),--features test-fault-guard,--features test-fault-supervisor)
endif
endif

KERNEL_ELF := $(BUILD)/indianos.elf
ISO := $(BUILD)/indianos.iso
ISO_ROOT := $(BUILD)/iso_root
LIMINE_BINARY := $(BUILD)/third_party/limine-binary
LIMINE_TARBALL := $(BUILD)/third_party/limine-binary.tar.gz
# Immutable release from https://github.com/Limine-Bootloader/Limine/releases/tag/v12.9.3
LIMINE_URL := https://github.com/Limine-Bootloader/Limine/releases/download/v12.9.3/limine-binary.tar.gz
LIMINE_SHA256 := 9f42fe9ea2e84d71056529969b1d5c24ba9a2287ffcc01bcce58b0ef8aa98b1d

SOURCES := kernel/src/main.c kernel/src/serial.c kernel/src/boot.c kernel/src/pmm.c kernel/src/vmm.c kernel/src/vmm_boot.c kernel/src/paging_clone.c kernel/src/paging_policy.c kernel/src/program.c kernel/src/syscall_abi.c kernel/src/memory_runtime.c kernel/src/initramfs.c kernel/src/user_image.c kernel/src/user_image_boot.c kernel/src/user_copy.c kernel/src/user_fault_policy.c kernel/src/process.c kernel/src/scheduler.c
ifeq ($(RUST_CORE),1)
# The three original C translation units remain for comparison / rollback,
# but are never linked alongside duplicate Rust C-ABI symbols.
SOURCES := $(filter-out kernel/src/process.c kernel/src/scheduler.c kernel/src/user_fault_policy.c kernel/src/syscall_abi.c kernel/src/program.c kernel/src/initramfs.c,$(SOURCES))
ifeq ($(RUST_VMM),1)
SOURCES := $(filter-out kernel/src/vmm.c,$(SOURCES))
endif
ifeq ($(RUST_PMM),1)
SOURCES := $(filter-out kernel/src/pmm.c,$(SOURCES))
endif
ifeq ($(RUST_USER_IMAGE),1)
SOURCES := $(filter-out kernel/src/user_image.c kernel/src/user_copy.c,$(SOURCES))
endif
ifeq ($(RUST_PAGING),1)
SOURCES := $(filter-out kernel/src/paging_policy.c kernel/src/paging_clone.c,$(SOURCES))
endif
ifeq ($(RUST_RUNTIME),1)
SOURCES := $(filter-out kernel/src/serial.c kernel/src/memory_runtime.c,$(SOURCES))
endif
ifeq ($(RUST_BOOT_STAGING),1)
SOURCES := $(filter-out kernel/src/vmm_boot.c kernel/src/user_image_boot.c,$(SOURCES))
endif
ifeq ($(RUST_BOOT_PROTOCOL),1)
# Request markers and response getters are emitted by Rust, not boot.c.
SOURCES := $(filter-out kernel/src/boot.c,$(SOURCES))
endif
endif
ifeq ($(RUST_MAIN),1)
SOURCES := $(filter-out kernel/src/main.c,$(SOURCES))
endif
OBJECTS := $(patsubst kernel/src/%.c,$(BUILD)/obj/%.o,$(SOURCES))
# The standalone assembly objects exist only for the reference/rollback build.
ifneq ($(RUST_CPU_ENTRY),1)
OBJECTS += $(BUILD)/obj/interrupt_entry.o $(BUILD)/obj/user_entry.o
endif
ifneq ($(RUST_USER_RUNTIME),1)
OBJECTS += $(BUILD)/obj/user_mode.o
endif
ifneq ($(RUST_PAGING_CPU),1)
OBJECTS += $(BUILD)/obj/paging.o
endif
ifneq ($(RUST_TIMER),1)
OBJECTS += $(BUILD)/obj/timer.o
endif
ifneq ($(RUST_INTERRUPTS),1)
OBJECTS += $(BUILD)/obj/interrupts.o
endif
ifeq ($(RUST_CORE),1)
OBJECTS += $(RUST_LIB)
endif
HEADERS := $(wildcard kernel/include/indianos/*.h) third_party/limine/limine.h

CFLAGS := -target x86_64-unknown-none-elf -std=c11 -O2 -g -Wall -Wextra -Werror \
          -ffreestanding -fno-stack-protector -fno-stack-check \
          -fno-pic -fno-pie -fno-plt -fno-builtin -fno-common \
          -fno-asynchronous-unwind-tables -fno-unwind-tables \
          -m64 -march=x86-64 -mno-mmx -mno-sse -mno-red-zone -mcmodel=kernel \
          -I kernel/include -I third_party/limine
ifeq ($(RUST_CORE),1)
CFLAGS += -DINDIANOS_RUST_CORE
endif
ifeq ($(RUST_BOOT),1)
CFLAGS += -DINDIANOS_RUST_BOOT
endif

ifeq ($(EXCEPTION_SELFTEST),1)
CFLAGS += -DINDIANOS_TEST_UD2
endif
ifeq ($(PAGEFAULT_SELFTEST),1)
CFLAGS += -DINDIANOS_TEST_PAGEFAULT
endif
ifeq ($(PAGING_SWITCH_SELFTEST),1)
CFLAGS += -DINDIANOS_TEST_CR3_SWITCH
endif
ifeq ($(USERMODE_SELFTEST),1)
CFLAGS += -DINDIANOS_TEST_RING3
endif
ifeq ($(SCHEDULER_SELFTEST),1)
CFLAGS += -DINDIANOS_TEST_SCHEDULER
endif
ifeq ($(PREEMPT_SELFTEST),1)
CFLAGS += -DINDIANOS_TEST_PREEMPT
endif
ifeq ($(PROCESS_SELFTEST),1)
CFLAGS += -DINDIANOS_TEST_PROCESSES
endif
USERFAULT_KIND ?= supervisor
ifeq ($(USERFAULT_SELFTEST),1)
ifeq ($(USERFAULT_KIND),guard)
CFLAGS += -DINDIANOS_TEST_USERFAULT_GUARD
else ifeq ($(USERFAULT_KIND),supervisor)
CFLAGS += -DINDIANOS_TEST_USERFAULT_SUPERVISOR
else
$(error USERFAULT_KIND must be guard or supervisor)
endif
endif
# Export all actual link inputs as ld.lld map provenance for R010 audit.
LDFLAGS := -target x86_64-unknown-none-elf -fuse-ld=lld -nostdlib -static \
           -Wl,-z,max-page-size=0x1000 -Wl,-z,noexecstack \
           -Wl,--gc-sections -Wl,--build-id=none -Wl,-T,kernel/linker.ld \
           -Wl,-Map,$(BUILD)/kernel-link.map

.PHONY: test-r026-interactive-qemu test-r025-complete-qemu test-rust-ipc-qemu test-rust-keyboard-host test-rust-keyboard-qemu test-rust-interactive-host test-rust-interactive-qemu test-rust-shell-host test-rust-shell-qemu test-rust-hardening test-rust-entry-abi test-rust-link-audit test-rust-source-audit-negative test-rust-source-audit test-rust-default-qemu test-rust-tools test-rust-userland-qemu test-rust-interrupts-qemu test-rust-boot-qemu test-rust-vmm-host test-rust-vmm-qemu test-rust-pmm-host test-rust-pmm-qemu test-rust-user-image-host test-rust-user-image-qemu test-rust-paging-host test-rust-paging-qemu test-rust-runtime-host test-rust-runtime-qemu test-rust-boot-staging-qemu test-rust-host-abi test-rust-processes test-rust-scheduler rust-core all kernel user-elf test-user-elf test-ring3-elf initramfs test-initramfs image run run-bios test-boot test-exception test-pagefault test-shadow test-usermode test-userfault-supervisor test-userfault-guard test-processes test-scheduler test-preempt test-host toolchain-check clean
all: image
kernel: $(KERNEL_ELF)

# Freestanding Rust: no_std, no allocator and no crates.io dependencies.
# A separate Cargo output root is used per build variant. RUST_LIB is last
# in link order. On the default path this single archive also supplies CPU
# exception, GDT, IRETQ and IRQ0 stubs via Rust-managed global_asm.
$(RUST_LIB): $(RUST_MANIFEST) $(RUST_SOURCE) Makefile
	@mkdir -p $(@D)
	CARGO_TARGET_DIR=$(BUILD)/rust-target \
	RUSTFLAGS="-C relocation-model=static -C code-model=kernel" \
	    $(CARGO) build --manifest-path $(RUST_MANIFEST) \
	    --target x86_64-unknown-none --release --offline --no-default-features $(RUST_FEATURES)
	cp $(BUILD)/rust-target/x86_64-unknown-none/release/libindianos_kernel_core.a $@

rust-core: $(RUST_LIB)

# Ensure the default linked kernel consists entirely of Rust archive members.
# The hardware instructions remain low-level assembly, embedded by global_asm.
# Original .S reference objects may only be linked in explicit rollback builds.
test-rust-source-audit:
	@test "$(RUST_CORE)" = "1" || (echo "Rust kernel is disabled" >&2; exit 1)
	@test "$(RUST_PAGING_CPU)" = "1" && test "$(RUST_USER_RUNTIME)" = "1" && test "$(RUST_CPU_ENTRY)" = "1" || (echo "Rust CPU/runtime/entry migration is disabled" >&2; exit 1)
	@test -z "$(strip $(SOURCES))" || (echo "Unexpected active kernel C: $(SOURCES)" >&2; exit 1)
	@test "$(words $(OBJECTS))" -eq 1 && test "$(strip $(OBJECTS))" = "$(RUST_LIB)" || (echo "Unexpected standalone objects: $(OBJECTS)" >&2; exit 1)
	@echo "IndianOS R011: default kernel links only the Rust static archive."

# Refuse any C translation units on the Rust-default path. This negative
# test intentionally turns on legacy flags; the child audit MUST fail.
test-rust-source-audit-negative:
	@if $(MAKE) --no-print-directory -s test-rust-source-audit RUST_CORE=0 >/dev/null 2>&1; then \
	    echo "R010 FAIL: old C kernel passed Rust source audit" >&2; exit 1; fi
	@if $(MAKE) --no-print-directory -s test-rust-source-audit RUST_MAIN=0 >/dev/null 2>&1; then \
	    echo "R010 FAIL: old C main passed Rust source audit" >&2; exit 1; fi
	@if $(MAKE) --no-print-directory -s test-rust-source-audit RUST_USER_RUNTIME=0 >/dev/null 2>&1; then \
	    echo "R010 FAIL: old C runtime passed Rust source audit" >&2; exit 1; fi
	@if $(MAKE) --no-print-directory -s test-rust-source-audit RUST_INTERRUPTS=0 >/dev/null 2>&1; then \
	    echo "R010 FAIL: old C interrupt handler passed Rust source audit" >&2; exit 1; fi
	@if $(MAKE) --no-print-directory -s test-rust-source-audit RUST_CPU_ENTRY=0 >/dev/null 2>&1; then \
	    echo "R011 FAIL: standalone assembly entry objects passed Rust audit" >&2; exit 1; fi
	@echo "IndianOS R011: negative C/assembly source audit PASSED"

# Verify actual ELF link origin: no standalone assembly or C objects.
test-rust-link-audit: test-rust-source-audit $(KERNEL_ELF) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) audit-kernel $(BUILD)/kernel-link.map $(KERNEL_ELF) $(RUST_LIB)

# Verify all stable CPU ABI symbols still exist in the Rust-archive-backed
# kernel. nm is included with the binutils installed by kernel CI.
test-rust-entry-abi: test-rust-link-audit
	@nm -g --defined-only $(KERNEL_ELF) > $(BUILD)/kernel-symbols.txt
	@for sym in x86_64_load_gdt x86_64_exception_stubs \
	    x86_64_isr_0 x86_64_isr_8 x86_64_isr_14 x86_64_isr_31 \
	    x86_64_unexpected_interrupt x86_64_enter_user x86_64_resume_user \
	    x86_64_user_interrupt x86_64_user_exit_to_kernel \
	    x86_64_timer_interrupt indianos_user_return_rsp indianos_user_return_cr3; do \
	    awk -v name="$$sym" '$$3==name {ok=1} END {exit !ok}' \
	        $(BUILD)/kernel-symbols.txt || \
	        { echo "R011 missing CPU ABI symbol: $$sym" >&2; exit 1; }; \
	done
	@echo "IndianOS R011: all exported CPU ABI symbols preserved."

# R012 host-only hostile CPU-frame, page-permission and copy-ownership tests.
test-rust-hardening:
	CARGO_TARGET_DIR=build/rust-hardening-target $(CARGO) test \
	    --manifest-path $(RUST_MANIFEST) --offline --features user-image

# R013: run native Rust shell grammar and syscall allowlist unit tests.
test-rust-shell-host:
	@mkdir -p build/r013-shell-host
	rustc --edition=2021 --test userland/rust/tests/shell_commands.rs \
	    -o build/r013-shell-host/command_tests
	build/r013-shell-host/command_tests
	CARGO_TARGET_DIR=build/r013-kernel-host $(CARGO) test \
	    --manifest-path $(RUST_MANIFEST) --offline --features test-shell

test-rust-host-abi:
	@mkdir -p build/rust-tests
	$(CARGO) test --manifest-path $(RUST_MANIFEST) --offline
	CARGO_TARGET_DIR=build/rust-tests/host $(CARGO) build --manifest-path $(RUST_MANIFEST) \
	    --release --offline --no-default-features
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_process.c tests/rust_host_personality.c \
	    build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/process_test
	build/rust-tests/process_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_scheduler.c tests/rust_host_personality.c \
	    build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/scheduler_test
	build/rust-tests/scheduler_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_user_fault.c tests/rust_host_personality.c \
	    build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/user_fault_test
	build/rust-tests/user_fault_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_program.c tests/rust_host_personality.c \
	    build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/program_test
	build/rust-tests/program_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_user_image.c kernel/src/user_image.c kernel/src/user_copy.c \
	    kernel/src/vmm.c tests/rust_host_personality.c \
	    build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/user_image_test
	build/rust-tests/user_image_test

test-rust-smoke:
	$(CARGO) test --manifest-path $(RUST_TOOLS_MANIFEST) --offline
	$(MAKE) image BUILD=build/rust-smoke-normal
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) smoke --iso build/rust-smoke-normal/indianos.iso \
	    --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-smoke-process PROCESS_SELFTEST=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-smoke-process/indianos.iso \
	    --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes

test-rust-tools:
	$(CARGO) test --manifest-path $(RUST_TOOLS_MANIFEST) --offline
	$(MAKE) test-initramfs BUILD=build/rust-tools-archive PROCESS_SELFTEST=1 RUST_TOOLS=1
	python3 tools/build_initramfs.py \
	    --elf build/rust-tools-archive/userland/demo.elf --name /bin/init \
	    --extra-elf build/rust-tools-archive/userland/worker.elf \
	    --extra-name /bin/worker \
	    --output build/rust-tools-archive/python-reference.img
	cmp build/rust-tools-archive/initramfs.img build/rust-tools-archive/python-reference.img
	$(MAKE) test-ring3-elf BUILD=build/rust-tools-elf USERMODE_SELFTEST=1
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) check-elf build/ring3/userland/demo.elf

test-rust-userland-qemu:
	$(MAKE) image BUILD=build/rust-user-inert RUST_USERLAND=1
	python3 tools/smoke_boot.py --iso build/rust-user-inert/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) test-ring3-elf BUILD=build/rust-user-demo USERMODE_SELFTEST=1 RUST_USERLAND=1
	$(MAKE) image BUILD=build/rust-user-demo USERMODE_SELFTEST=1 RUST_USERLAND=1
	python3 tools/smoke_boot.py --iso build/rust-user-demo/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode
	$(MAKE) test-initramfs BUILD=build/rust-user-dual PROCESS_SELFTEST=1 RUST_USERLAND=1
	$(MAKE) image BUILD=build/rust-user-dual PROCESS_SELFTEST=1 RUST_USERLAND=1
	python3 tools/smoke_boot.py --iso build/rust-user-dual/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-user-sched SCHEDULER_SELFTEST=1 RUST_USERLAND=1
	python3 tools/smoke_boot.py --iso build/rust-user-sched/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler
	$(MAKE) image BUILD=build/rust-user-guard USERFAULT_SELFTEST=1 USERFAULT_KIND=guard RUST_USERLAND=1
	python3 tools/smoke_boot.py --iso build/rust-user-guard/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-guard
	$(MAKE) image BUILD=build/rust-user-supervisor USERFAULT_SELFTEST=1 USERFAULT_KIND=supervisor RUST_USERLAND=1
	python3 tools/smoke_boot.py --iso build/rust-user-supervisor/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-supervisor

test-rust-timer-link-qemu:
	$(MAKE) image BUILD=build/rust-timer-normal RUST_TIMER=1
	python3 tools/smoke_boot.py --iso build/rust-timer-normal/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-timer-user RUST_TIMER=1 USERMODE_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-timer-user/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode

# Explicit acceptance check for the integrated default (no opt-in flags).
# Both cases exercise Rust Limine requests + kmain + IDT/TSS + timer link.
test-rust-ring3-runtime-qemu:
	$(MAKE) image BUILD=build/rust-runtime-normal RUST_USER_RUNTIME=1
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) smoke --iso build/rust-runtime-normal/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-runtime-processes RUST_USER_RUNTIME=1 PROCESS_SELFTEST=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-runtime-processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-runtime-scheduler RUST_USER_RUNTIME=1 SCHEDULER_SELFTEST=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-runtime-scheduler/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-rust-paging-cpu-qemu:
	$(MAKE) image BUILD=build/rust-paging-cpu RUST_CORE=1 RUST_PAGING_CPU=1
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) smoke --iso build/rust-paging-cpu/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-paging-user RUST_CORE=1 RUST_PAGING_CPU=1 USERMODE_SELFTEST=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-paging-user/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode

# Boot the scripted shell at CPL3, authorize one known worker and reap both.
test-rust-shell-qemu:
	$(MAKE) image BUILD=build/r013-shell R013_SHELL_TEST=1 RUST_CORE=1
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) smoke --iso build/r013-shell/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode shell

# Real input in COM1 and optional PS/2, via QEMU injected serial commands.
test-rust-interactive-host:
	CARGO_TARGET_DIR=build/r014-input-host $(CARGO) test \
	    --manifest-path $(RUST_MANIFEST) --offline --features test-interactive

test-rust-interactive-qemu:
	$(MAKE) image BUILD=build/r014-interactive R014_INTERACTIVE_TEST=1 RUST_CORE=1
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) smoke --iso build/r014-interactive/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode interactive --timeout 45

# R014.2 PS/2 IRQ1, ring-3 IF=1, and Limine framebuffer terminal.
test-rust-keyboard-host:
	CARGO_TARGET_DIR=build/r0142-host $(CARGO) test \
	    --manifest-path $(RUST_MANIFEST) --offline --features test-irq-keyboard

test-rust-keyboard-qemu:
	$(MAKE) image BUILD=build/r0142-keyboard R014_IRQ_TEST=1 RUST_CORE=1
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) smoke --iso build/r0142-keyboard/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode keyboard --machine pc --timeout 65

test-rust-default-qemu:
	$(MAKE) image BUILD=build/rust-default-normal RUST_CORE=1
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) smoke --iso build/rust-default-normal/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-default-ring3 USERMODE_SELFTEST=1 RUST_CORE=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-default-ring3/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode

test-rust-integration-qemu:
	$(MAKE) $(RUST_TOOLS_BIN)
	$(MAKE) image BUILD=build/rust-all-normal RUST_BOOT=1 RUST_INTERRUPTS=1 RUST_TIMER=1 RUST_USERLAND=1 RUST_TOOLS=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-all-normal/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-all-ring3 USERMODE_SELFTEST=1 RUST_BOOT=1 RUST_INTERRUPTS=1 RUST_TIMER=1 RUST_USERLAND=1 RUST_TOOLS=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-all-ring3/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode
	$(MAKE) image BUILD=build/rust-all-guard USERFAULT_SELFTEST=1 USERFAULT_KIND=guard RUST_BOOT=1 RUST_INTERRUPTS=1 RUST_TIMER=1 RUST_USERLAND=1 RUST_TOOLS=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-all-guard/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-guard
	$(MAKE) image BUILD=build/rust-all-sched SCHEDULER_SELFTEST=1 RUST_BOOT=1 RUST_INTERRUPTS=1 RUST_TIMER=1 RUST_USERLAND=1 RUST_TOOLS=1
	$(RUST_TOOLS_BIN) smoke --iso build/rust-all-sched/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-rust-main-qemu:
	$(MAKE) image BUILD=build/rust-main-normal RUST_MAIN=1 RUST_BOOT=1 RUST_INTERRUPTS=1
	python3 tools/smoke_boot.py --iso build/rust-main-normal/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-main-ud2 RUST_MAIN=1 RUST_BOOT=1 RUST_INTERRUPTS=1 EXCEPTION_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-main-ud2/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode exception
	$(MAKE) image BUILD=build/rust-main-pf RUST_MAIN=1 RUST_BOOT=1 RUST_INTERRUPTS=1 PAGEFAULT_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-main-pf/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode pagefault
	$(MAKE) image BUILD=build/rust-main-process RUST_MAIN=1 RUST_BOOT=1 RUST_INTERRUPTS=1 PROCESS_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-main-process/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-main-sched RUST_MAIN=1 RUST_BOOT=1 RUST_INTERRUPTS=1 SCHEDULER_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-main-sched/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler
	$(MAKE) image BUILD=build/rust-main-guard RUST_MAIN=1 RUST_BOOT=1 RUST_INTERRUPTS=1 USERFAULT_SELFTEST=1 USERFAULT_KIND=guard
	python3 tools/smoke_boot.py --iso build/rust-main-guard/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-guard

test-rust-interrupts-qemu:
	$(MAKE) image BUILD=build/rust-idt-normal RUST_INTERRUPTS=1
	python3 tools/smoke_boot.py --iso build/rust-idt-normal/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-idt-ud2 RUST_INTERRUPTS=1 EXCEPTION_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-idt-ud2/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode exception
	$(MAKE) image BUILD=build/rust-idt-fault RUST_INTERRUPTS=1 PAGEFAULT_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-idt-fault/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode pagefault
	$(MAKE) image BUILD=build/rust-idt-user RUST_INTERRUPTS=1 USERMODE_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-idt-user/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode
	$(MAKE) image BUILD=build/rust-idt-guard RUST_INTERRUPTS=1 USERFAULT_SELFTEST=1 USERFAULT_KIND=guard
	python3 tools/smoke_boot.py --iso build/rust-idt-guard/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-guard
	$(MAKE) image BUILD=build/rust-idt-sched RUST_INTERRUPTS=1 SCHEDULER_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/rust-idt-sched/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-rust-boot-qemu:
	$(MAKE) image BUILD=build/rust-boot-image RUST_BOOT=1
	python3 tools/smoke_boot.py --iso build/rust-boot-image/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-boot-process PROCESS_SELFTEST=1 RUST_BOOT=1
	python3 tools/smoke_boot.py --iso build/rust-boot-process/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-boot-fault USERFAULT_SELFTEST=1 USERFAULT_KIND=guard RUST_BOOT=1
	python3 tools/smoke_boot.py --iso build/rust-boot-fault/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-guard

test-rust-boot-staging-qemu:
	$(MAKE) image BUILD=build/rust-boot-staging RUST_CORE=1 RUST_PMM=1 RUST_VMM=1 RUST_USER_IMAGE=1 RUST_BOOT_STAGING=1
	python3 tools/smoke_boot.py --iso build/rust-boot-staging/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-boot-staging-process PROCESS_SELFTEST=1 RUST_CORE=1 RUST_PMM=1 RUST_VMM=1 RUST_USER_IMAGE=1 RUST_BOOT_STAGING=1
	python3 tools/smoke_boot.py --iso build/rust-boot-staging-process/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes

test-rust-runtime-host:
	@mkdir -p build/rust-runtime-host
	CARGO_TARGET_DIR=build/rust-runtime-host/target $(CARGO) build \
	    --manifest-path $(RUST_MANIFEST) --release --offline \
	    --no-default-features --features runtime
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -fno-builtin \
	    -I kernel/include tests/test_rust_memory.c tests/rust_host_personality.c \
	    build/rust-runtime-host/target/release/libindianos_kernel_core.a \
	    -o build/rust-runtime-host/runtime_test
	build/rust-runtime-host/runtime_test

test-rust-runtime-qemu:
	$(MAKE) image BUILD=build/rust-runtime-boot RUST_CORE=1 RUST_RUNTIME=1
	python3 tools/smoke_boot.py --iso build/rust-runtime-boot/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode normal
	$(MAKE) image BUILD=build/rust-runtime-processes PROCESS_SELFTEST=1 RUST_CORE=1 RUST_RUNTIME=1
	python3 tools/smoke_boot.py --iso build/rust-runtime-processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes

test-rust-paging-host:
	@mkdir -p build/rust-paging-host
	CARGO_TARGET_DIR=build/rust-paging-host/target $(CARGO) build \
	    --manifest-path $(RUST_MANIFEST) --release --offline \
	    --no-default-features --features paging
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_paging.c tests/rust_host_personality.c \
	    build/rust-paging-host/target/release/libindianos_kernel_core.a \
	    -o build/rust-paging-host/paging_test
	build/rust-paging-host/paging_test

test-rust-paging-qemu:
	$(MAKE) image BUILD=build/rust-paging-processes PROCESS_SELFTEST=1 RUST_CORE=1 RUST_PAGING=1
	python3 tools/smoke_boot.py --iso build/rust-paging-processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-paging-fault USERFAULT_SELFTEST=1 USERFAULT_KIND=supervisor RUST_CORE=1 RUST_PAGING=1
	python3 tools/smoke_boot.py --iso build/rust-paging-fault/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-supervisor

test-rust-user-image-host:
	@mkdir -p build/rust-user-image-host
	CARGO_TARGET_DIR=build/rust-user-image-host/target $(CARGO) build \
	    --manifest-path $(RUST_MANIFEST) --release --offline \
	    --no-default-features --features user-image
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_user_image.c tests/rust_host_personality.c \
	    build/rust-user-image-host/target/release/libindianos_kernel_core.a \
	    -o build/rust-user-image-host/user_image_test
	build/rust-user-image-host/user_image_test

test-rust-user-image-qemu:
	$(MAKE) image BUILD=build/rust-image-processes PROCESS_SELFTEST=1 RUST_CORE=1 RUST_VMM=1 RUST_USER_IMAGE=1
	python3 tools/smoke_boot.py --iso build/rust-image-processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-image-scheduler SCHEDULER_SELFTEST=1 RUST_CORE=1 RUST_VMM=1 RUST_USER_IMAGE=1
	python3 tools/smoke_boot.py --iso build/rust-image-scheduler/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-rust-pmm-host:
	@mkdir -p build/rust-pmm-host
	CARGO_TARGET_DIR=build/rust-pmm-host/target $(CARGO) build \
	    --manifest-path $(RUST_MANIFEST) --release --offline \
	    --no-default-features --features pmm
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_pmm.c tests/rust_host_personality.c \
	    build/rust-pmm-host/target/release/libindianos_kernel_core.a \
	    -o build/rust-pmm-host/pmm_test
	build/rust-pmm-host/pmm_test

test-rust-pmm-qemu:
	$(MAKE) image BUILD=build/rust-pmm-processes PROCESS_SELFTEST=1 RUST_CORE=1 RUST_PMM=1
	python3 tools/smoke_boot.py --iso build/rust-pmm-processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-pmm-scheduler SCHEDULER_SELFTEST=1 RUST_CORE=1 RUST_PMM=1
	python3 tools/smoke_boot.py --iso build/rust-pmm-scheduler/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-rust-vmm-host:
	@mkdir -p build/rust-vmm-host
	CARGO_TARGET_DIR=build/rust-vmm-host/target $(CARGO) build \
	    --manifest-path $(RUST_MANIFEST) --release --offline \
	    --no-default-features --features vmm
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_vmm.c tests/rust_host_personality.c \
	    build/rust-vmm-host/target/release/libindianos_kernel_core.a \
	    -o build/rust-vmm-host/vmm_test
	build/rust-vmm-host/vmm_test

test-rust-vmm-qemu:
	$(MAKE) image BUILD=build/rust-vmm-processes PROCESS_SELFTEST=1 RUST_CORE=1 RUST_VMM=1
	python3 tools/smoke_boot.py --iso build/rust-vmm-processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes
	$(MAKE) image BUILD=build/rust-vmm-scheduler SCHEDULER_SELFTEST=1 RUST_CORE=1 RUST_VMM=1
	python3 tools/smoke_boot.py --iso build/rust-vmm-scheduler/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-rust-processes:
	$(MAKE) test-initramfs BUILD=build/rust-processes PROCESS_SELFTEST=1 RUST_CORE=1
	$(MAKE) image BUILD=build/rust-processes PROCESS_SELFTEST=1 RUST_CORE=1
	python3 tools/smoke_boot.py --iso build/rust-processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes

test-rust-scheduler:
	$(MAKE) test-initramfs BUILD=build/rust-scheduler SCHEDULER_SELFTEST=1 RUST_CORE=1
	$(MAKE) image BUILD=build/rust-scheduler SCHEDULER_SELFTEST=1 RUST_CORE=1
	python3 tools/smoke_boot.py --iso build/rust-scheduler/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

$(BUILD)/obj/%.o: kernel/src/%.c $(HEADERS) Makefile
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/obj/paging.o: kernel/arch/x86_64/paging.c $(HEADERS) Makefile
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/obj/timer.o: kernel/arch/x86_64/timer.c $(HEADERS) Makefile
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/obj/user_mode.o: kernel/arch/x86_64/user_mode.c $(HEADERS) Makefile
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/obj/user_entry.o: kernel/arch/x86_64/user_entry.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone \
	    $(if $(filter 1,$(PREEMPT_SELFTEST)),-DINDIANOS_TEST_PREEMPT,) -c $< -o $@

$(BUILD)/obj/interrupts.o: kernel/arch/x86_64/interrupts.c $(HEADERS) Makefile
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/obj/interrupt_entry.o: kernel/arch/x86_64/interrupt_entry.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@

$(KERNEL_ELF): $(OBJECTS) kernel/linker.ld Makefile
	@mkdir -p $(@D)
	$(CC) $(LDFLAGS) $(OBJECTS) -o $@
	@readelf -h $@ | grep -q 'Machine:.*Advanced Micro Devices X86-64'
	@echo 'IndianOS: x86-64 ELF kernel compiled.'

USER_ELF := $(BUILD)/userland/demo.elf
USER_OBJ := $(BUILD)/userland/entry.o
WORKER_OBJ := $(BUILD)/userland/worker.o
WORKER_ELF := $(BUILD)/userland/worker.elf
INITRAMFS := $(BUILD)/initramfs.img
ifneq ($(filter 1,$(PROCESS_SELFTEST) $(SCHEDULER_SELFTEST) $(R013_SHELL_TEST) $(R014_INTERACTIVE_TEST)),)
INITRAMFS_EXTRA_DEPS := $(WORKER_ELF)
INITRAMFS_EXTRA_ARGS := --extra-elf $(WORKER_ELF) --extra-name /bin/worker
endif

ifeq ($(RUST_USERLAND),1)
# Identical expected serial messages and syscall ABI; no C/assembly
# objects are linked into the Rust native user executable.
ifeq ($(R030_PACKAGE_V1),1)
RUST_INIT_FEATURE := r030-demo-v1
else ifeq ($(R030_PACKAGE_V2),1)
RUST_INIT_FEATURE := r030-demo-v2
else ifeq ($(R029_USERSPACE_TEST),1)
RUST_INIT_FEATURE := r029-client
else ifeq ($(R026_INTERACTIVE_TEST),1)
RUST_INIT_FEATURE := r026-shell
RUST_WORKER_FEATURE := r026-worker
else ifeq ($(R025_COMPLETE_TEST),1)
RUST_INIT_FEATURE := r025-a
RUST_WORKER_FEATURE := r025-b
else ifeq ($(R025_IPC_TEST),1)
RUST_INIT_FEATURE := ipc-a
RUST_WORKER_FEATURE := ipc-b
else ifeq ($(R014_INTERACTIVE_TEST),1)
RUST_INIT_FEATURE := interactive
RUST_WORKER_FEATURE := worker
else ifeq ($(R013_SHELL_TEST),1)
RUST_INIT_FEATURE := shell
RUST_WORKER_FEATURE := worker
else ifeq ($(USERFAULT_SELFTEST),1)
RUST_INIT_FEATURE := fault-$(USERFAULT_KIND)
else ifeq ($(PREEMPT_SELFTEST),1)
RUST_INIT_FEATURE := preempt-a
RUST_WORKER_FEATURE := preempt-b
else ifeq ($(SCHEDULER_SELFTEST),1)
RUST_INIT_FEATURE := sched-a
RUST_WORKER_FEATURE := sched-b
else ifeq ($(PROCESS_SELFTEST),1)
RUST_INIT_FEATURE := demo
RUST_WORKER_FEATURE := worker
else ifeq ($(USERMODE_SELFTEST),1)
RUST_INIT_FEATURE := demo
else
RUST_INIT_FEATURE :=
endif
RUST_APP_FLAGS := -C relocation-model=static -C link-arg=-T$(CURDIR)/userland/demo/linker.ld -C link-arg=--no-pie -C link-arg=-z -C link-arg=max-page-size=0x1000
$(USER_ELF): $(wildcard userland/rust/src/*.rs) userland/rust/Cargo.toml userland/rust/Cargo.lock userland/demo/linker.ld Makefile
	@mkdir -p $(@D)
	CARGO_TARGET_DIR=$(BUILD)/rust-user-init-target \
	    RUSTFLAGS="$(RUST_APP_FLAGS)" \
	    $(CARGO) build --manifest-path userland/rust/Cargo.toml \
	    --target x86_64-unknown-none --release --offline --no-default-features \
	    $(if $(RUST_INIT_FEATURE),--features $(RUST_INIT_FEATURE),)
	cp $(BUILD)/rust-user-init-target/x86_64-unknown-none/release/indianos-user $@
	@readelf -h $@ | grep -q 'Type:.*EXEC'

$(WORKER_ELF): $(wildcard userland/rust/src/*.rs) userland/rust/Cargo.toml userland/rust/Cargo.lock userland/demo/linker.ld Makefile
	@mkdir -p $(@D)
	CARGO_TARGET_DIR=$(BUILD)/rust-user-worker-target \
	    RUSTFLAGS="$(RUST_APP_FLAGS)" \
	    $(CARGO) build --manifest-path userland/rust/Cargo.toml \
	    --target x86_64-unknown-none --release --offline --no-default-features \
	    --features $(RUST_WORKER_FEATURE)
	cp $(BUILD)/rust-user-worker-target/x86_64-unknown-none/release/indianos-user $@
	@readelf -h $@ | grep -q 'Type:.*EXEC'

else
# The normal ISO retains the inert fixture; only test-usermode replaces it
# with a separate app that uses the draft IndianOS software interrupt ABI.
ifeq ($(USERFAULT_SELFTEST),1)
$(USER_OBJ): userland/demo/fault_entry.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone \
	    $(if $(filter guard,$(USERFAULT_KIND)),-DINDIANOS_TEST_FAULT_GUARD,) \
	    -c $< -o $@
else ifeq ($(PREEMPT_SELFTEST),1)
$(USER_OBJ): userland/demo/preempt_a.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@
else ifeq ($(SCHEDULER_SELFTEST),1)
$(USER_OBJ): userland/demo/scheduler_a.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@
else ifeq ($(PROCESS_SELFTEST),1)
$(USER_OBJ): userland/demo/ring3_entry.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@
else ifeq ($(USERMODE_SELFTEST),1)
$(USER_OBJ): userland/demo/ring3_entry.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@
else
$(USER_OBJ): userland/demo/entry.c Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -std=c11 -O2 -ffreestanding \
	    -fno-builtin -fno-pie -fno-pic -fno-stack-protector -mno-red-zone \
	    -c $< -o $@
endif

$(USER_ELF): $(USER_OBJ) userland/demo/linker.ld Makefile
	$(CC) -target x86_64-unknown-none-elf -fuse-ld=lld -nostdlib -static \
	    -Wl,-z,max-page-size=0x1000 -Wl,-z,noexecstack \
	    -Wl,-T,userland/demo/linker.ld $(USER_OBJ) -o $@
	@readelf -h $@ | grep -q 'Type:.*EXEC'

ifeq ($(PREEMPT_SELFTEST),1)
$(WORKER_OBJ): userland/demo/preempt_b.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@
else ifeq ($(SCHEDULER_SELFTEST),1)
$(WORKER_OBJ): userland/demo/scheduler_b.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@
else
$(WORKER_OBJ): userland/demo/worker_entry.S Makefile
	@mkdir -p $(@D)
	$(CC) -target x86_64-unknown-none-elf -mno-red-zone -c $< -o $@
endif

$(WORKER_ELF): $(WORKER_OBJ) userland/demo/linker.ld Makefile
	$(CC) -target x86_64-unknown-none-elf -fuse-ld=lld -nostdlib -static \
	    -Wl,-z,max-page-size=0x1000 -Wl,-z,noexecstack \
	    -Wl,-T,userland/demo/linker.ld $(WORKER_OBJ) -o $@
	@readelf -h $@ | grep -q 'Type:.*EXEC'

endif # RUST_USERLAND

$(RUST_TOOLS_BIN): $(wildcard tools/rust/src/*.rs) tools/rust/Cargo.toml tools/rust/Cargo.lock Makefile
	CARGO_TARGET_DIR=$(BUILD)/rust-tools-target $(CARGO) build \
	    --manifest-path $(RUST_TOOLS_MANIFEST) --release --offline
	@test -f $@

user-elf: $(USER_ELF)

$(BUILD)/inspect_elf: tests/inspect_elf.c kernel/src/program.c kernel/include/indianos/program.h Makefile
	@mkdir -p $(@D)
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/inspect_elf.c kernel/src/program.c -o $@

test-user-elf: $(USER_ELF) $(BUILD)/inspect_elf
	$(BUILD)/inspect_elf $(USER_ELF)

ifeq ($(RUST_TOOLS),1)
$(INITRAMFS): $(USER_ELF) $(INITRAMFS_EXTRA_DEPS) $(RUST_TOOLS_BIN) Makefile
	$(RUST_TOOLS_BIN) pack --elf $(USER_ELF) --name /bin/init \
	    $(INITRAMFS_EXTRA_ARGS) --output $@
else
$(INITRAMFS): $(USER_ELF) $(INITRAMFS_EXTRA_DEPS) tools/build_initramfs.py Makefile
	python3 tools/build_initramfs.py --elf $(USER_ELF) --name /bin/init \
	    $(INITRAMFS_EXTRA_ARGS) --output $@
endif

initramfs: $(INITRAMFS)

$(BUILD)/inspect_archive: tests/inspect_archive.c kernel/src/initramfs.c kernel/src/program.c $(HEADERS) Makefile
	@mkdir -p $(@D)
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/inspect_archive.c kernel/src/initramfs.c kernel/src/program.c -o $@

test-initramfs: $(INITRAMFS) $(BUILD)/inspect_archive
	$(BUILD)/inspect_archive $(INITRAMFS) $(if $(filter 1,$(PROCESS_SELFTEST) $(SCHEDULER_SELFTEST)),/bin/worker,)

toolchain-check:
	@command -v $(CC) && command -v ld.lld && command -v readelf
	@echo 'Compiler, linker, and ELF reader available.'

$(LIMINE_TARBALL):
	@mkdir -p $(@D)
	curl -fL --retry 3 -o $@.tmp $(LIMINE_URL)
	@printf '%s  %s\n' '$(LIMINE_SHA256)' '$@.tmp' | sha256sum -c -
	@mv $@.tmp $@

$(LIMINE_BINARY)/limine.c: $(LIMINE_TARBALL)
	@mkdir -p $(BUILD)/third_party
	@rm -rf $(LIMINE_BINARY)
	@tar -xzf $(LIMINE_TARBALL) -C $(BUILD)/third_party
	@test -f $@

$(LIMINE_BINARY)/limine: $(LIMINE_BINARY)/limine.c
	$(MAKE) -C $(LIMINE_BINARY) limine

$(ISO): $(KERNEL_ELF) $(INITRAMFS) $(LIMINE_BINARY)/limine boot/limine.conf
	@command -v xorriso >/dev/null || (echo 'Missing xorriso: install package xorriso' >&2; exit 1)
	@rm -rf $(ISO_ROOT)
	@mkdir -p $(ISO_ROOT)/boot/limine $(ISO_ROOT)/EFI/BOOT
	@cp $(KERNEL_ELF) $(ISO_ROOT)/boot/indianos.elf
	@cp $(INITRAMFS) $(ISO_ROOT)/boot/initramfs.img
	@cp boot/limine.conf $(ISO_ROOT)/boot/limine/limine.conf
	@cp $(LIMINE_BINARY)/limine-bios.sys $(LIMINE_BINARY)/limine-bios-cd.bin \
	    $(LIMINE_BINARY)/limine-uefi-cd.bin $(ISO_ROOT)/boot/limine/
	@cp $(LIMINE_BINARY)/BOOTX64.EFI $(ISO_ROOT)/EFI/BOOT/
	xorriso -as mkisofs -R -r -J -b boot/limine/limine-bios-cd.bin \
	    -no-emul-boot -boot-load-size 4 -boot-info-table \
	    --efi-boot boot/limine/limine-uefi-cd.bin -efi-boot-part \
	    --efi-boot-image --protective-msdos-label $(ISO_ROOT) -o $(ISO)
	$(LIMINE_BINARY)/limine bios-install $(ISO)
	@echo 'Bootable image: $(ISO) (EMULATOR TESTING ONLY)'

image: $(ISO)

run: $(ISO)
	@test -f "$(OVMF_CODE)" || (echo 'Cannot find OVMF firmware; set OVMF_CODE=/path/to/OVMF_CODE*.fd' >&2; exit 1)
	$(QEMU) -machine q35 -m 512M -drive if=pflash,unit=0,format=raw,file="$(OVMF_CODE)",readonly=on \
	    -cdrom $(ISO) -boot d -serial stdio -monitor none -no-reboot

run-bios: $(ISO)
	$(QEMU) -machine q35 -m 512M -cdrom $(ISO) -boot d -serial stdio -monitor none -no-reboot

test-boot: $(ISO) $(SMOKE_DEPS)
	$(SMOKE) --iso $(ISO) --qemu $(QEMU) --ovmf "$(OVMF_CODE)"

test-exception:
	$(MAKE) image BUILD=build/ud2 EXCEPTION_SELFTEST=1
	$(SMOKE) --iso build/ud2/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode exception

test-pagefault:
	$(MAKE) image BUILD=build/pagefault PAGEFAULT_SELFTEST=1
	$(SMOKE) --iso build/pagefault/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode pagefault

test-shadow:
	$(MAKE) image BUILD=build/shadow PAGING_SWITCH_SELFTEST=1
	$(SMOKE) --iso build/shadow/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode shadow

test-ring3-elf:
	$(MAKE) user-elf BUILD=build/ring3 USERMODE_SELFTEST=1
ifeq ($(RUST_TOOLS),1)
	$(MAKE) $(RUST_TOOLS_BIN)
	$(RUST_TOOLS_BIN) check-elf build/ring3/userland/demo.elf
else
	python3 tools/check_usermode_fixture.py build/ring3/userland/demo.elf
endif
	$(MAKE) test-initramfs BUILD=build/ring3 USERMODE_SELFTEST=1

test-usermode: test-ring3-elf
	$(MAKE) image BUILD=build/ring3 USERMODE_SELFTEST=1
	$(SMOKE) --iso build/ring3/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode

test-userfault-supervisor:
	$(MAKE) image BUILD=build/fault-supervisor USERFAULT_SELFTEST=1 USERFAULT_KIND=supervisor
	$(SMOKE) --iso build/fault-supervisor/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-supervisor

test-userfault-guard:
	$(MAKE) image BUILD=build/fault-guard USERFAULT_SELFTEST=1 USERFAULT_KIND=guard
	$(SMOKE) --iso build/fault-guard/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-guard

test-processes:
	$(MAKE) test-initramfs BUILD=build/processes PROCESS_SELFTEST=1
	$(MAKE) image BUILD=build/processes PROCESS_SELFTEST=1
	$(SMOKE) --iso build/processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes

test-scheduler:
	$(MAKE) test-initramfs BUILD=build/scheduler SCHEDULER_SELFTEST=1
	$(MAKE) image BUILD=build/scheduler SCHEDULER_SELFTEST=1
	$(SMOKE) --iso build/scheduler/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-r026-interactive-qemu: $(SMOKE_DEPS)
	$(MAKE) test-initramfs BUILD=build/r026-interactive R026_INTERACTIVE_TEST=1
	$(MAKE) image BUILD=build/r026-interactive R026_INTERACTIVE_TEST=1
	$(SMOKE) --iso build/r026-interactive/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode r026 --timeout 60

test-r025-complete-qemu: $(SMOKE_DEPS)
	$(MAKE) test-initramfs BUILD=build/r025-complete R025_COMPLETE_TEST=1
	$(MAKE) image BUILD=build/r025-complete R025_COMPLETE_TEST=1
	$(SMOKE) --iso build/r025-complete/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode r025 --timeout 40

test-rust-ipc-qemu: $(SMOKE_DEPS)
	$(MAKE) test-initramfs BUILD=build/r025-ipc R025_IPC_TEST=1
	$(MAKE) image BUILD=build/r025-ipc R025_IPC_TEST=1
	$(SMOKE) --iso build/r025-ipc/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode ipc --timeout 35

test-preempt: $(SMOKE_DEPS)
	$(MAKE) test-initramfs BUILD=build/preempt SCHEDULER_SELFTEST=1 PREEMPT_SELFTEST=1
	$(MAKE) image BUILD=build/preempt SCHEDULER_SELFTEST=1 PREEMPT_SELFTEST=1
	$(SMOKE) --iso build/preempt/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode preempt --machine pc --timeout 35

test-host:
	@mkdir -p $(BUILD)
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_pmm.c kernel/src/pmm.c -o $(BUILD)/pmm_test
	$(BUILD)/pmm_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_vmm.c kernel/src/vmm.c -o $(BUILD)/vmm_test
	$(BUILD)/vmm_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_paging.c kernel/src/paging_policy.c kernel/src/paging_clone.c -o $(BUILD)/paging_test
	$(BUILD)/paging_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_program.c kernel/src/program.c kernel/src/syscall_abi.c -o $(BUILD)/program_test
	$(BUILD)/program_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_user_image.c kernel/src/program.c kernel/src/initramfs.c \
	    kernel/src/user_image.c kernel/src/user_copy.c kernel/src/vmm.c -o $(BUILD)/user_image_test
	$(BUILD)/user_image_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_user_fault.c kernel/src/user_fault_policy.c -o $(BUILD)/user_fault_test
	$(BUILD)/user_fault_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_process.c kernel/src/process.c -o $(BUILD)/process_test
	$(BUILD)/process_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_scheduler.c kernel/src/scheduler.c kernel/src/process.c \
	    -o $(BUILD)/scheduler_test
	$(BUILD)/scheduler_test
	python3 -m unittest discover -s tests -v

clean:
	rm -rf $(BUILD)

# R015: only a disposable temporary disk; never attach a host block device.
.PHONY: test-rust-ata-qemu
.PHONY: test-r029-user-surfaces-qemu

test-r029-user-surfaces-qemu:
	$(MAKE) image BUILD=build/r029-user-surfaces RUST_CORE=1 R029_USERSPACE_TEST=1
	python3 tools/test_r029_user_surfaces_qemu.py --iso build/r029-user-surfaces/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)"

.PHONY: test-r030-package-qemu

test-r030-package-qemu:
	python3 -m unittest discover -s tests -p 'test_r030_pkg.py' -v
	python3 tools/test_r030_package_qemu.py --qemu $(QEMU) --ovmf "$(OVMF_CODE)"

.PHONY: test-r029-desktop-qemu

test-r029-desktop-qemu:
	$(MAKE) image BUILD=build/r029-desktop RUST_CORE=1 R029_DESKTOP_TEST=1
	python3 tools/test_r029_desktop_qemu.py --iso build/r029-desktop/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)"

.PHONY: test-r028-network-qemu

test-r028-network-qemu:
	$(MAKE) image BUILD=build/r028-network RUST_CORE=1 R028_NETWORK_TEST=1
	python3 tools/test_r028_network_qemu.py --iso build/r028-network/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)"

.PHONY: test-r027-durable-qemu

test-r027-durable-qemu:
	$(MAKE) image BUILD=build/r027-durable RUST_CORE=1 R027_DURABLE_TEST=1
	python3 tools/test_r027_reboot_qemu.py --iso build/r027-durable/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)"

test-rust-ata-qemu:
	$(MAKE) image BUILD=build/r015-ata RUST_CORE=1 R015_ATA_TEST=1
	python3 tools/test_ata_qemu.py --iso build/r015-ata/indianos.iso --qemu "$(QEMU)" --ovmf "$(OVMF_CODE)"
