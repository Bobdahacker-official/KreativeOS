//! Native x86-64 user programs with separate mappings and an interrupt ABI.
use crate::{hardware, text::Text};
use core::{
    arch::{asm, global_asm},
    fmt::Write,
    mem::size_of,
    ptr,
};

const BASE: usize = 0x4000_0000_0000;
const PAGE: usize = 4096;
const STACK: usize = BASE + 3 * PAGE;
const PRESENT: u64 = 1;
const WRITE: u64 = 2;
const USER: u64 = 4;
const NX: u64 = 1 << 63;
const ADDR: u64 = 0x000f_ffff_ffff_f000;
const TIMER: usize = 0xf1;
const INTERRUPT: usize = 0x80;

#[repr(C, align(4096))]
struct Page([u64; 512]);
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Table {
    limit: u16,
    base: u64,
}
#[repr(C, align(16))]
struct KernelStack([u8; 16384]);
#[repr(C, align(16))]
struct FxState([u8; 512]);

static mut PML4: Page = Page([0; 512]);
static mut PDPT: Page = Page([0; 512]);
static mut PD: Page = Page([0; 512]);
static mut PT: Page = Page([0; 512]);
static mut CODE: Page = Page([0; 512]);
static mut DATA: Page = Page([0; 512]);
static mut GDT: Page = Page([0; 512]);
static mut TSS: [u8; 104] = [0; 104];
static mut IDT: [u128; 256] = [0; 256];
static mut KSTACK: KernelStack = KernelStack([0; 16384]);
static mut FRAME: usize = 0;
static mut SELECTORS: Option<(u64, u64)> = None;
static mut OUTPUT: Text<1024> = Text::new();
static mut KERNEL_FP: FxState = FxState([0; 512]);
static mut USER_FP: FxState = FxState([0; 512]);

extern "C" {
    fn kos_user_syscall();
    fn kos_user_fault();
    fn kos_user_timer();
}
extern "win64" {
    fn kos_user_enter(cr3: u64, cs: u64, ss: u64) -> u64;
}

global_asm!(
    ".text",
    ".globl kos_user_enter",
    "kos_user_enter:",
    // Win64 ABI. Only volatile registers are modified on this path.
    "fxsave64 [rip + {kernel_fp}]",
    "fxrstor64 [rip + {user_fp}]",
    "mov cr3, rcx",
    "push r8",
    "mov rax, {stack_top}",
    "push rax",
    "push 0x202",
    "push rdx",
    "mov rax, {entry}",
    "push rax",
    // No register may expose values left by the kernel caller.
    "push rbx", "push rbp", "push rdi", "push rsi",
    "push r12", "push r13", "push r14", "push r15",
    "mov [rip + {frame}], rsp",
    // Move the IRET frame below the saved kernel registers.
    "push qword ptr [rsp + 96]",
    "push qword ptr [rsp + 96]",
    "push qword ptr [rsp + 96]",
    "push qword ptr [rsp + 96]",
    "push qword ptr [rsp + 96]",
    "xor eax, eax", "xor ecx, ecx", "xor edx, edx",
    "xor ebx, ebx", "xor ebp, ebp", "xor edi, edi", "xor esi, esi",
    "xor r8d, r8d", "xor r9d, r9d", "xor r10d, r10d", "xor r11d, r11d",
    "xor r12d, r12d", "xor r13d, r13d", "xor r14d, r14d", "xor r15d, r15d",
    "iretq",
    ".globl kos_user_syscall",
    "kos_user_syscall:",
    "cld",
    "cmp eax, 1",
    "je 2f",
    "test eax, eax",
    "jne 3f",
    "mov ecx, ebx",
    "mov edx, 0",
    "jmp 5f",
    "3:",
    "mov rax, -38",
    "iretq",
    "2:",
    "mov rcx, rbx",
    "mov rdx, rsi",
    // TSS stack is aligned, with five hardware words pushed by the CPU.
    "sub rsp, 40",
    "call {write}",
    "add rsp, 40",
    "iretq",
    ".globl kos_user_fault",
    "kos_user_fault:",
    "cld",
    "xor ecx, ecx",
    "mov edx, 1",
    "jmp 5f",
    ".globl kos_user_timer",
    "kos_user_timer:",
    "cld",
    "xor ecx, ecx",
    "mov edx, 2",
    "5:",
    "mov rsp, [rip + {frame}]",
    "fxrstor64 [rip + {kernel_fp}]",
    "mov eax, ecx",
    "mov ecx, edx",
    "shl rcx, 32",
    "or rax, rcx",
    "pop r15", "pop r14", "pop r13", "pop r12",
    "pop rsi", "pop rdi", "pop rbp", "pop rbx",
    "add rsp, 40",
    "ret",
    frame = sym FRAME,
    kernel_fp = sym KERNEL_FP,
    user_fp = sym USER_FP,
    write = sym syscall_write,
    stack_top = const (STACK + PAGE),
    entry = const BASE,
);

extern "win64" fn syscall_write(address: usize, len: usize) -> i64 {
    let end = address.saturating_add(len);
    let readable =
        (address >= BASE && end <= BASE + PAGE) || (address >= STACK && end <= STACK + PAGE);
    if !readable || len > 512 {
        return -14;
    }
    let data = unsafe { core::slice::from_raw_parts(address as *const u8, len) };
    unsafe {
        let available = OUTPUT.bytes.len() - OUTPUT.len;
        for &byte in data.iter().take(available) {
            OUTPUT.push(if byte == b'\n' || (32..127).contains(&byte) {
                byte
            } else {
                b'?'
            });
        }
        len.min(available) as i64
    }
}

unsafe fn rdmsr(index: u32) -> u64 {
    let (low, high): (u32, u32);
    asm!("rdmsr", in("ecx") index, out("eax") low, out("edx") high, options(nostack));
    (u64::from(high) << 32) | u64::from(low)
}

unsafe fn wrmsr(index: u32, value: u64) {
    asm!("wrmsr", in("ecx") index, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nostack));
}

struct Apic(u64);
impl Apic {
    unsafe fn read(&self, offset: u32) -> u32 {
        if self.0 & (1 << 10) != 0 {
            rdmsr(0x800 + offset / 16) as u32
        } else {
            ptr::read_volatile(((self.0 & ADDR) + u64::from(offset)) as *const u32)
        }
    }
    unsafe fn write(&self, offset: u32, value: u32) {
        if self.0 & (1 << 10) != 0 {
            wrmsr(0x800 + offset / 16, u64::from(value));
        } else {
            ptr::write_volatile(((self.0 & ADDR) + u64::from(offset)) as *mut u32, value);
        }
    }
}

unsafe fn user_segments() -> Option<(u64, u64)> {
    if let Some(selectors) = SELECTORS {
        return Some(selectors);
    }
    let mut original = Table { limit: 0, base: 0 };
    asm!("sgdt [{}]", in(reg) &mut original, options(nostack));
    let old = usize::from(original.limit) + 1;
    let next = old.div_ceil(8);
    if next + 4 > 512 {
        return None;
    }
    ptr::copy_nonoverlapping(original.base as *const u8, GDT.0.as_mut_ptr().cast(), old);
    GDT.0[next] = 0x00af_fa00_0000_ffff; // User executable, long mode.
    GDT.0[next + 1] = 0x00cf_f200_0000_ffff; // User data.
    let tss = TSS.as_ptr() as u64;
    let ring0_top = KSTACK.0.as_mut_ptr().add(KSTACK.0.len()) as u64;
    TSS[4..12].copy_from_slice(&ring0_top.to_le_bytes());
    TSS[102..104].copy_from_slice(&(TSS.len() as u16).to_le_bytes());
    GDT.0[next + 2] =
        103 | ((tss & 0x00ff_ffff) << 16) | (0x89 << 40) | (((tss >> 24) & 0xff) << 56);
    GDT.0[next + 3] = tss >> 32;
    let table = Table {
        limit: ((next + 4) * 8 - 1) as u16,
        base: GDT.0.as_ptr() as u64,
    };
    asm!("lgdt [{}]", "ltr {selector:x}", in(reg) &table, selector = in(reg) ((next + 2) * 8) as u16, options(nostack));
    let selectors = ((next * 8 | 3) as u64, ((next + 1) * 8 | 3) as u64);
    SELECTORS = Some(selectors);
    Some(selectors)
}

fn gate(handler: usize, cs: u16, user: bool) -> u128 {
    let address = handler as u128;
    (address & 0xffff)
        | (u128::from(cs) << 16)
        | (if user { 0xee } else { 0x8e } << 40)
        | ((address & 0xffff_0000) << 32)
        | ((address >> 32) << 64)
}

pub fn run(binary: &[u8], out: &mut impl Write) -> Result<(), &'static str> {
    let program = binary
        .strip_prefix(b"KR3\x01")
        .filter(|body| !body.is_empty() && body.len() <= PAGE)
        .ok_or("Invalid KR3 executable")?;
    unsafe {
        if core::arch::x86_64::__cpuid(1).edx & (1 << 9) == 0
            || core::arch::x86_64::__cpuid(0x8000_0001).edx & (1 << 20) == 0
        {
            return Err("Local APIC and NX support are required");
        }
        let (cr3, cr4): (u64, u64);
        asm!("mov {}, cr3", "mov {}, cr4", out(reg) cr3, out(reg) cr4, options(nostack));
        if cr4 & ((1 << 12) | (1 << 21) | (1 << 23)) != 0 {
            return Err("This firmware paging or CPU protection configuration is unsupported");
        }
        let apic = Apic(rdmsr(0x1b));
        if apic.0 & (1 << 11) == 0 {
            return Err("Local APIC is disabled");
        }
        let root = &*((cr3 & ADDR) as *const [u64; 512]);
        let index = BASE >> 39;
        if root[index] & PRESENT != 0 {
            return Err("User address range is already mapped");
        }
        let (cs_user, ss_user) = user_segments().ok_or("No GDT space available")?;
        for (dest, &original) in PML4.0.iter_mut().zip(root) {
            *dest = original & !USER;
        }
        PML4.0[index] = PDPT.0.as_ptr() as u64 | PRESENT | WRITE | USER;
        PDPT.0[0] = PD.0.as_ptr() as u64 | PRESENT | WRITE | USER;
        PD.0[0] = PT.0.as_ptr() as u64 | PRESENT | WRITE | USER;
        PT.0[0] = CODE.0.as_ptr() as u64 | PRESENT | USER;
        PT.0[3] = DATA.0.as_ptr() as u64 | PRESENT | WRITE | USER | NX;
        CODE.0.fill(0);
        DATA.0.fill(0);
        ptr::copy_nonoverlapping(program.as_ptr(), CODE.0.as_mut_ptr().cast(), program.len());
        OUTPUT.clear();
        USER_FP.0[..2].copy_from_slice(&0x037fu16.to_le_bytes());
        USER_FP.0[24..28].copy_from_slice(&0x1f80u32.to_le_bytes());

        let mut old_idt = Table { limit: 0, base: 0 };
        let cs: u16;
        asm!("sidt [{}]", "mov {cs:x}, cs", in(reg) &mut old_idt, cs = out(reg) cs, options(nostack));
        IDT.fill(gate(kos_user_fault as *const () as usize, cs, false));
        IDT[INTERRUPT] = gate(kos_user_syscall as *const () as usize, cs, true);
        IDT[TIMER] = gate(kos_user_timer as *const () as usize, cs, false);
        let idt = Table {
            limit: (size_of::<[u128; 256]>() - 1) as u16,
            base: IDT.as_ptr() as u64,
        };
        let saved_efer = rdmsr(0xc000_0080);
        let has_sysenter = core::arch::x86_64::__cpuid(1).edx & (1 << 11) != 0;
        let sysenter = if has_sysenter { rdmsr(0x174) } else { 0 };
        let fs = rdmsr(0xc000_0100);
        let gs = rdmsr(0xc000_0101);
        let (fs_selector, gs_selector): (u16, u16);
        asm!("mov {0:x}, fs", "mov {1:x}, gs", out(reg) fs_selector, out(reg) gs_selector, options(nostack));
        let svr = apic.read(0xf0);
        let lvt = apic.read(0x320);
        let divide = apic.read(0x3e0);
        let count = apic.read(0x390);
        apic.write(0xf0, svr | 0x100);
        apic.write(0x320, TIMER as u32 | (1 << 16));
        apic.write(0x3e0, 3); // Divide the APIC timer clock by 16.
        apic.write(0x380, u32::MAX);
        let before = hardware::millis();
        while hardware::millis() < before + 4 {
            core::hint::spin_loop();
        }
        let ticks = u32::MAX - apic.read(0x390);
        if ticks == 0 {
            apic.write(0x320, lvt);
            apic.write(0x3e0, divide);
            apic.write(0x380, count);
            apic.write(0xf0, svr);
            return Err("APIC timer could not be calibrated");
        }
        wrmsr(0xc000_0080, (saved_efer | (1 << 11)) & !1); // NX on; SYSCALL disabled.
        if has_sysenter {
            wrmsr(0x174, 0);
        }
        // Flush global firmware translations and restrict user arithmetic to the saved x87/SSE state.
        let user_cr4 = cr4 & !((1 << 7) | (1 << 16) | (1 << 18));
        asm!("mov cr4, {}", "lidt [{}]", in(reg) user_cr4, in(reg) &idt, options(nostack));
        apic.write(0x320, TIMER as u32);
        apic.write(0x380, ticks.saturating_mul(63));
        let result = kos_user_enter(PML4.0.as_ptr() as u64, cs_user, ss_user);
        asm!("cli", "mov cr3, {}", "lidt [{}]", in(reg) cr3, in(reg) &old_idt, options(nostack));
        apic.write(0x320, lvt | (1 << 16));
        apic.write(0x380, 0);
        if result >> 32 == 2 {
            apic.write(0xb0, 0);
        }
        apic.write(0x3e0, divide);
        apic.write(0x320, lvt);
        apic.write(0x380, count);
        apic.write(0xf0, svr);
        asm!("mov cr4, {}", in(reg) cr4, options(nostack));
        wrmsr(0xc000_0080, saved_efer);
        if has_sysenter {
            wrmsr(0x174, sysenter);
        }
        asm!("mov fs, {0:x}", "mov gs, {1:x}", in(reg) fs_selector, in(reg) gs_selector, options(nostack));
        wrmsr(0xc000_0100, fs);
        wrmsr(0xc000_0101, gs);
        let _ = write!(out, "{}", OUTPUT.str());
        match result >> 32 {
            0 => {
                let mut status = Text::<48>::new();
                let _ = write!(status, "KreativeOS: user exit {}\r\n", result as u32);
                hardware::log(status.str().as_bytes());
                let _ = writeln!(out, "\nUser process exited ({})", result as u32);
            }
            1 => {
                hardware::log(b"KreativeOS: user CPU exception\r\n");
                let _ = writeln!(out, "\nUser process stopped on a CPU exception");
            }
            _ => {
                hardware::log(b"KreativeOS: user CPU time limit\r\n");
                let _ = writeln!(out, "\nUser process reached its CPU time limit");
            }
        }
    }
    Ok(())
}
