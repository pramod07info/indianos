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
# Rust migration is explicitly opt-in until QEMU/ABI regression verifies it.
RUST_CORE ?= 0
CARGO ?= cargo
RUST_MANIFEST := rust/kernel_core/Cargo.toml
RUST_SOURCE := rust/kernel_core/src/lib.rs
RUST_LIB := $(BUILD)/libindianos_kernel_core.a
RUST_FEATURES := $(if $(filter 1,$(PREEMPT_SELFTEST)),--features preempt,)
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
SOURCES := $(filter-out kernel/src/process.c kernel/src/scheduler.c kernel/src/user_fault_policy.c,$(SOURCES))
endif
OBJECTS := $(patsubst kernel/src/%.c,$(BUILD)/obj/%.o,$(SOURCES))
OBJECTS += $(BUILD)/obj/interrupts.o $(BUILD)/obj/interrupt_entry.o $(BUILD)/obj/paging.o $(BUILD)/obj/user_mode.o $(BUILD)/obj/user_entry.o $(BUILD)/obj/timer.o
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
LDFLAGS := -target x86_64-unknown-none-elf -fuse-ld=lld -nostdlib -static \
           -Wl,-z,max-page-size=0x1000 -Wl,-z,noexecstack \
           -Wl,--gc-sections -Wl,--build-id=none -Wl,-T,kernel/linker.ld

.PHONY: test-rust-host-abi test-rust-processes test-rust-scheduler rust-core all kernel user-elf test-user-elf test-ring3-elf initramfs test-initramfs image run run-bios test-boot test-exception test-pagefault test-shadow test-usermode test-userfault-supervisor test-userfault-guard test-processes test-scheduler test-preempt test-host toolchain-check clean
all: image
kernel: $(KERNEL_ELF)

# Freestanding Rust: no_std, no allocator and no crates.io dependencies.
# A separate Cargo output root is used per build variant. RUST_LIB is last
# in the kernel link order, after C and CPU entry stubs that reference it.
$(RUST_LIB): $(RUST_MANIFEST) $(RUST_SOURCE) Makefile
	@mkdir -p $(@D)
	CARGO_TARGET_DIR=$(BUILD)/rust-target $(CARGO) build --manifest-path $(RUST_MANIFEST) \
	    --target x86_64-unknown-none --release --offline $(RUST_FEATURES)
	cp $(BUILD)/rust-target/x86_64-unknown-none/release/libindianos_kernel_core.a $@

rust-core: $(RUST_LIB)

test-rust-host-abi:
	@mkdir -p build/rust-tests
	$(CARGO) test --manifest-path $(RUST_MANIFEST) --offline
	CARGO_TARGET_DIR=build/rust-tests/host $(CARGO) build --manifest-path $(RUST_MANIFEST) \
	    --release --offline
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_process.c build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/process_test
	build/rust-tests/process_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_scheduler.c build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/scheduler_test
	build/rust-tests/scheduler_test
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/test_user_fault.c build/rust-tests/host/release/libindianos_kernel_core.a \
	    -o build/rust-tests/user_fault_test
	build/rust-tests/user_fault_test

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
ifneq ($(filter 1,$(PROCESS_SELFTEST) $(SCHEDULER_SELFTEST)),)
INITRAMFS_EXTRA_DEPS := $(WORKER_ELF)
INITRAMFS_EXTRA_ARGS := --extra-elf $(WORKER_ELF) --extra-name /bin/worker
endif

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

user-elf: $(USER_ELF)

$(BUILD)/inspect_elf: tests/inspect_elf.c kernel/src/program.c kernel/include/indianos/program.h Makefile
	@mkdir -p $(@D)
	$(CC) -std=c11 -O2 -g -Wall -Wextra -Werror -I kernel/include \
	    tests/inspect_elf.c kernel/src/program.c -o $@

test-user-elf: $(USER_ELF) $(BUILD)/inspect_elf
	$(BUILD)/inspect_elf $(USER_ELF)

$(INITRAMFS): $(USER_ELF) $(INITRAMFS_EXTRA_DEPS) tools/build_initramfs.py Makefile
	python3 tools/build_initramfs.py --elf $(USER_ELF) --name /bin/init \
	    $(INITRAMFS_EXTRA_ARGS) --output $@

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

test-boot: $(ISO)
	python3 tools/smoke_boot.py --iso $(ISO) --qemu $(QEMU) --ovmf "$(OVMF_CODE)"

test-exception:
	$(MAKE) image BUILD=build/ud2 EXCEPTION_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/ud2/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode exception

test-pagefault:
	$(MAKE) image BUILD=build/pagefault PAGEFAULT_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/pagefault/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode pagefault

test-shadow:
	$(MAKE) image BUILD=build/shadow PAGING_SWITCH_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/shadow/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode shadow

test-ring3-elf:
	$(MAKE) user-elf BUILD=build/ring3 USERMODE_SELFTEST=1
	python3 tools/check_usermode_fixture.py build/ring3/userland/demo.elf
	$(MAKE) test-initramfs BUILD=build/ring3 USERMODE_SELFTEST=1

test-usermode: test-ring3-elf
	$(MAKE) image BUILD=build/ring3 USERMODE_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/ring3/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode usermode

test-userfault-supervisor:
	$(MAKE) image BUILD=build/fault-supervisor USERFAULT_SELFTEST=1 USERFAULT_KIND=supervisor
	python3 tools/smoke_boot.py --iso build/fault-supervisor/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-supervisor

test-userfault-guard:
	$(MAKE) image BUILD=build/fault-guard USERFAULT_SELFTEST=1 USERFAULT_KIND=guard
	python3 tools/smoke_boot.py --iso build/fault-guard/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode fault-guard

test-processes:
	$(MAKE) test-initramfs BUILD=build/processes PROCESS_SELFTEST=1
	$(MAKE) image BUILD=build/processes PROCESS_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/processes/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode processes

test-scheduler:
	$(MAKE) test-initramfs BUILD=build/scheduler SCHEDULER_SELFTEST=1
	$(MAKE) image BUILD=build/scheduler SCHEDULER_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/scheduler/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode scheduler

test-preempt:
	$(MAKE) test-initramfs BUILD=build/preempt SCHEDULER_SELFTEST=1 PREEMPT_SELFTEST=1
	$(MAKE) image BUILD=build/preempt SCHEDULER_SELFTEST=1 PREEMPT_SELFTEST=1
	python3 tools/smoke_boot.py --iso build/preempt/indianos.iso --qemu $(QEMU) --ovmf "$(OVMF_CODE)" --mode preempt --machine pc --timeout 35

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
