//! Native x86-64 user programs with separate mappings and an interrupt ABI.
use crate::{hardware, text::Text};
use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::{
    arch::{asm, global_asm},
    fmt::Write,
    mem::size_of,
    ptr,
};

const BASE: usize = 0x4000_0000_0000;
const PAGE: usize = 4096;
const CODE_LIMIT: usize = 16 * PAGE;
const STACK: usize = BASE + 20 * PAGE;
const STACK_PAGES: usize = 4;
const HEAP: usize = BASE + 32 * PAGE;
const HEAP_PAGES: usize = 128;
const STACK_TOP: usize = STACK + STACK_PAGES * PAGE;
const JOB_SLOT: usize = 18;
const JOB_ADDRESS: usize = BASE + JOB_SLOT * PAGE;
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

static mut GDT: Page = Page([0; 512]);
static mut TSS: [u8; 104] = [0; 104];
static mut IDT: [u128; 256] = [0; 256];
static mut KSTACK: KernelStack = KernelStack([0; 16384]);
static mut FRAME: usize = 0;
static mut ACTIVE_CONTEXT: *mut [u64; 20] = ptr::null_mut();
static mut ACTIVE_PROCESS: *mut Process = ptr::null_mut();
static mut APIC_TICKS: u32 = 0;
static mut SELECTORS: Option<(u64, u64)> = None;
static mut KERNEL_FP: FxState = FxState([0; 512]);
static mut USER_FP: FxState = FxState([0; 512]);

extern "C" {
    fn kos_user_syscall();
    fn kos_user_fault();
    fn kos_user_timer();
    static kos_maintenance_job: u8;
    static kos_maintenance_job_end: u8;
}
extern "win64" {
    fn kos_user_enter(cr3: u64, context: *const [u64; 20]) -> u64;
}

global_asm!(
    ".text",
    ".globl kos_user_enter",
    "kos_user_enter:",
    "fxsave64 [rip + {kernel_fp}]",
    "fxrstor64 [rip + {user_fp}]",
    "mov cr3, rcx",
    "push rbx", "push rbp", "push rdi", "push rsi",
    "push r12", "push r13", "push r14", "push r15",
    "mov [rip + {frame}], rsp",
    // Context is ordered as fifteen registers, followed by the five IRET words.
    "lea rax, [rdx + 152]",
    "mov ecx, 20",
    "1:",
    "push qword ptr [rax]",
    "sub rax, 8",
    "loop 1b",
    "pop r15", "pop r14", "pop r13", "pop r12", "pop r11", "pop r10", "pop r9", "pop r8",
    "pop rdi", "pop rsi", "pop rbp", "pop rbx", "pop rdx", "pop rcx", "pop rax",
    "iretq",
    ".globl kos_user_syscall",
    "kos_user_syscall:",
    "cld",
    "test eax, eax",
    "jz 2f",
    // The Windows ABI helper may use all volatile registers and SSE registers.
    // Preserve the user register file intact except for the syscall result in RAX.
    "push rcx", "push rdx", "push r8", "push r9", "push r10", "push r11",
    "fxsave64 [rip + {user_fp}]",
    "mov ecx, eax",
    "mov rdx, rbx",
    "mov r8, rsi",
    "sub rsp, 40",
    "call {syscall}",
    "add rsp, 40",
    "fxrstor64 [rip + {user_fp}]",
    "pop r11", "pop r10", "pop r9", "pop r8", "pop rdx", "pop rcx",
    "iretq",
    "2:",
    "mov ecx, ebx",
    "xor edx, edx",
    "jmp 5f",
    ".globl kos_user_fault",
    "kos_user_fault:",
    "cld",
    "xor ecx, ecx",
    "mov edx, 1",
    "jmp 5f",
    ".globl kos_user_timer",
    "kos_user_timer:",
    "cld",
    "push rax", "push rcx", "push rdx", "push rbx", "push rbp", "push rsi", "push rdi",
    "push r8", "push r9", "push r10", "push r11", "push r12", "push r13", "push r14", "push r15",
    "mov rsi, rsp",
    "mov rdi, [rip + {context}]",
    "mov ecx, 20",
    "rep movsq",
    "fxsave64 [rip + {user_fp}]",
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
    "ret",
    frame = sym FRAME,
    context = sym ACTIVE_CONTEXT,
    kernel_fp = sym KERNEL_FP,
    user_fp = sym USER_FP,
    syscall = sym user_syscall,
);

global_asm!(
    ".text",
    ".globl kos_maintenance_job",
    "kos_maintenance_job:",
    "mov rdx, {job}",
    "mov rax, [rdx + 8]",
    "cmp rax, [rdx + 16]",
    "jbe 1f",
    "mov [rdx + 16], rax",
    "1:",
    "movzx eax, byte ptr [rdx]",
    "test eax, eax",
    "jnz 3f",
    "mov eax, 1",
    "3:",
    "cmp eax, 8",
    "jbe 2f",
    "mov eax, 8",
    "2:",
    "mov byte ptr [rdx + 24], al",
    "xor eax, eax",
    "xor ebx, ebx",
    "int 0x80",
    ".globl kos_maintenance_job_end",
    "kos_maintenance_job_end:",
    job = const JOB_ADDRESS,
);

struct Process {
    id: u64,
    name: Text<32>,
    root: Box<Page>,
    pdpt: Box<Page>,
    pd: Box<Page>,
    table: Box<Page>,
    code: Vec<Box<Page>>,
    stack: Vec<Box<Page>>,
    job: Option<Box<Page>>,
    heap: Vec<Box<Page>>,
    context: [u64; 20],
    fp: FxState,
    output: Text<1024>,
}

impl Process {
    fn new(id: u64, name: &str, code: &[u8], job: Option<&[u8; 32]>) -> Box<Self> {
        let pages = code
            .chunks(PAGE)
            .map(|chunk| {
                let mut page = zero_page();
                page_bytes(&mut page)[..chunk.len()].copy_from_slice(chunk);
                page
            })
            .collect();
        let job = job.map(|state| {
            let mut page = zero_page();
            page_bytes(&mut page)[..state.len()].copy_from_slice(state);
            page
        });
        let mut process = Box::new(Self {
            id,
            name: Text::new(),
            root: zero_page(),
            pdpt: zero_page(),
            pd: zero_page(),
            table: zero_page(),
            code: pages,
            stack: (0..STACK_PAGES).map(|_| zero_page()).collect(),
            job,
            heap: Vec::new(),
            context: [0; 20],
            fp: FxState([0; 512]),
            output: Text::new(),
        });
        process.name.set(name);
        process.fp.0[..2].copy_from_slice(&0x037fu16.to_le_bytes());
        process.fp.0[24..28].copy_from_slice(&0x1f80u32.to_le_bytes());
        process.context[15] = BASE as u64;
        process.context[17] = 0x202;
        process.context[18] = STACK_TOP as u64;
        let rw = PRESENT | WRITE | USER;
        process.pdpt.0[0] = address(&process.pd) | rw;
        process.pd.0[0] = address(&process.table) | rw;
        for (slot, page) in process.code.iter().enumerate() {
            process.table.0[slot] = address(page) | PRESENT | USER;
        }
        for (slot, page) in process.stack.iter().enumerate() {
            process.table.0[(STACK - BASE) / PAGE + slot] = address(page) | rw | NX;
        }
        if let Some(page) = &process.job {
            process.table.0[JOB_SLOT] = address(page) | rw | NX;
        }
        process
    }

    fn grow_heap(&mut self, bytes: usize) -> i64 {
        if bytes > HEAP_PAGES * PAGE {
            return -12;
        }
        for index in self.heap.len()..bytes.div_ceil(PAGE) {
            let page = zero_page();
            self.table.0[(HEAP - BASE) / PAGE + index] =
                address(&page) | PRESENT | WRITE | USER | NX;
            self.heap.push(page);
            unsafe {
                asm!("invlpg [{}]", in(reg) HEAP + index * PAGE, options(nostack, preserves_flags))
            };
        }
        HEAP as i64
    }
}

fn zero_page() -> Box<Page> {
    Box::new(Page([0; 512]))
}

fn address(page: &Page) -> u64 {
    page.0.as_ptr() as u64
}

fn page_bytes(page: &mut Page) -> &mut [u8; PAGE] {
    unsafe { &mut *page.0.as_mut_ptr().cast() }
}

extern "win64" fn user_syscall(call: u32, first: usize, length: usize) -> i64 {
    let process = unsafe { &mut *ACTIVE_PROCESS };
    match call {
        1 => {
            let end = first.saturating_add(length);
            let readable = (first >= BASE && end <= BASE + process.code.len() * PAGE)
                || (first >= STACK && end <= STACK_TOP)
                || (first >= HEAP && end <= HEAP + process.heap.len() * PAGE);
            if !readable || length > 512 {
                return -14;
            }
            let data = unsafe { core::slice::from_raw_parts(first as *const u8, length) };
            let available = process.output.bytes.len() - process.output.len;
            for &byte in data.iter().take(available) {
                process
                    .output
                    .push(if byte == b'\n' || (32..127).contains(&byte) {
                        byte
                    } else {
                        b'?'
                    });
            }
            length.min(available) as i64
        }
        2 => process.grow_heap(first),
        _ => -38,
    }
}

#[derive(Clone, Copy)]
enum Slice {
    Exited(u32),
    Faulted,
    Preempted,
}

pub struct Scheduler {
    ready: VecDeque<Box<Process>>,
    next_id: u64,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            ready: VecDeque::new(),
            next_id: 1,
        }
    }

    pub fn spawn(&mut self, name: &str, binary: &[u8]) -> Result<u64, &'static str> {
        let code = binary
            .strip_prefix(b"KR3\x01")
            .filter(|code| !code.is_empty() && code.len() <= CODE_LIMIT)
            .ok_or("Invalid KR3 executable or code exceeds 64 KiB")?;
        if self.ready.len() >= 8 {
            return Err("The session already has eight active processes");
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Process identifiers exhausted")?;
        self.ready.push_back(Process::new(id, name, code, None));
        Ok(id)
    }

    pub fn list(&self, out: &mut impl Write) {
        if self.ready.is_empty() {
            let _ = writeln!(out, "No running programs");
        }
        for process in &self.ready {
            let _ = writeln!(
                out,
                "{}  {}  {} KiB heap",
                process.id,
                process.name.str(),
                process.heap.len() * 4
            );
        }
    }

    pub fn kill(&mut self, id: u64) -> bool {
        let index = self.ready.iter().position(|process| process.id == id);
        index.and_then(|index| self.ready.remove(index)).is_some()
    }

    pub fn clear(&mut self) {
        self.ready.clear();
    }

    pub fn step(&mut self, out: &mut Text<4096>) -> bool {
        let Some(mut process) = self.ready.pop_front() else {
            return false;
        };
        let result = run_slice(&mut process);
        let changed = !process.output.str().is_empty() || !matches!(result, Ok(Slice::Preempted));
        if changed && out.len > 3000 {
            out.clear();
        }
        if process.output.len != 0 {
            let _ = write!(out, "\n[{}] {}", process.id, process.output.str());
            process.output.clear();
        }
        match result {
            Ok(Slice::Preempted) => self.ready.push_back(process),
            Ok(Slice::Exited(status)) => {
                let _ = writeln!(out, "\n[{}] Exited ({status})", process.id);
                let mut line = Text::<64>::new();
                let _ = write!(line, "KreativeOS: user exit {status}\r\n");
                unsafe { hardware::log(line.str().as_bytes()) };
            }
            Ok(Slice::Faulted) => {
                let _ = writeln!(out, "\n[{}] Stopped on a CPU exception", process.id);
                unsafe { hardware::log(b"KreativeOS: user CPU exception\r\n") };
            }
            Err(error) => {
                let _ = writeln!(out, "\n[{}] {error}", process.id);
            }
        }
        changed
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

pub(crate) fn run_maintenance(state: &mut [u8; 32]) -> Result<(), &'static str> {
    unsafe {
        let first = ptr::addr_of!(kos_maintenance_job);
        let size = (ptr::addr_of!(kos_maintenance_job_end) as usize).saturating_sub(first as usize);
        if !(1..=PAGE).contains(&size) {
            return Err("Invalid built-in maintenance task");
        }
        let program = core::slice::from_raw_parts(first, size);
        let mut process = Process::new(0, "system1", program, Some(state));
        if matches!(run_slice(&mut process)?, Slice::Exited(0)) {
            state.copy_from_slice(&page_bytes(process.job.as_mut().unwrap())[..32]);
            Ok(())
        } else {
            Err("The maintenance task stopped before completing")
        }
    }
}

fn run_slice(process: &mut Process) -> Result<Slice, &'static str> {
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
        for (dest, &original) in process.root.0.iter_mut().zip(root) {
            *dest = original & !USER;
        }
        process.root.0[index] = address(&process.pdpt) | PRESENT | WRITE | USER;
        process.context[16] = cs_user;
        process.context[19] = ss_user;
        USER_FP.0.copy_from_slice(&process.fp.0);

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
        apic.write(0x3e0, 3);
        if APIC_TICKS == 0 {
            apic.write(0x380, u32::MAX);
            let before = hardware::millis();
            while hardware::millis() < before + 4 {
                core::hint::spin_loop();
            }
            APIC_TICKS = u32::MAX - apic.read(0x390);
        }
        if APIC_TICKS == 0 {
            apic.write(0x320, lvt);
            apic.write(0x3e0, divide);
            apic.write(0x380, count);
            apic.write(0xf0, svr);
            return Err("APIC timer could not be calibrated");
        }
        wrmsr(0xc000_0080, (saved_efer | (1 << 11)) & !1);
        if has_sysenter {
            wrmsr(0x174, 0);
        }
        let user_cr4 = cr4 & !((1 << 7) | (1 << 16) | (1 << 18));
        asm!("mov cr4, {}", "lidt [{}]", in(reg) user_cr4, in(reg) &idt, options(nostack));
        ACTIVE_CONTEXT = &mut process.context;
        ACTIVE_PROCESS = process;
        apic.write(0x320, TIMER as u32);
        apic.write(0x380, APIC_TICKS.saturating_mul(2));
        let result = kos_user_enter(address(&process.root), ptr::addr_of!(process.context));
        asm!("cli", "mov cr3, {}", "lidt [{}]", in(reg) cr3, in(reg) &old_idt, options(nostack));
        ACTIVE_PROCESS = ptr::null_mut();
        ACTIVE_CONTEXT = ptr::null_mut();
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
        Ok(match result >> 32 {
            0 => Slice::Exited(result as u32),
            1 => Slice::Faulted,
            _ => {
                process.fp.0.copy_from_slice(&USER_FP.0);
                Slice::Preempted
            }
        })
    }
}
