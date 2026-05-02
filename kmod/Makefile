obj-m += evo_raw.o

KDIR ?= /lib/modules/$(shell uname -r)/build

# Auto-detect Clang-built kernels (CachyOS, Arch, etc.) and enable LLVM toolchain.
# Callers can override: make LLVM=0 or make LLVM=1
_IS_CLANG := $(shell grep -c '^CONFIG_CC_IS_CLANG=y' $(KDIR)/.config 2>/dev/null)
LLVM      ?= $(_IS_CLANG)

all:
	$(MAKE) -C $(KDIR) M=$(CURDIR) LLVM=$(LLVM) modules

clean:
	$(MAKE) -C $(KDIR) M=$(CURDIR) LLVM=$(LLVM) clean

install:
	$(MAKE) -C $(KDIR) M=$(CURDIR) LLVM=$(LLVM) modules_install
	depmod -A
