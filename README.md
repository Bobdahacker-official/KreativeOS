# KreativeOS

KreativeOS is an independent operating system which has been developed by a community and is based on the 32-bit x86 architecture; it is not Windows or Linux, since it combines familiar desktop conventions with a small, inspectable and editable kernel and user interface.

## Current release

The desktop has these apps: Files, Notes, Paint, Terminal, Browser, Network, Calculator, Calendar, Stopwatch, Hex Viewer, Settings, Task Manager, Welcome, User Management, SysGuide, Update Manager, Package Installer, App Permissions, Notifications, Run, and the Recycle Bin. Optional KUPD packages consist of one enabling a boot using only the Terminal, System Monitor, Markdown Reader, Color Picker, Disk Benchmark, Quick Notes, and a polished setup beta. The packages are authenticated with HMAC-SHA-256 before being installed.

The system KFS3 uses two metadata journals and copy-on-write data banks; files can reach a size of 1 MiB, and when files are regularly deleted they are sent to the Recycle Bin in order that they may be recovered. Administrative approval is required for protected applications and system operations.

The version of [lwIP 2.2.1](https://github.com/lwip-tcpip/lwip) that is available under the BSD licence is used for networking.
source vendored under `third_party/lwip`; its copyright and license are retained
in that directory.

KreativeOS is a small, freestanding 32-bit x86 operating system written in C and
The NASM assembly boots directly from a BIOS floppy or hard-disk image and then switches to
When in protected mode it starts a VESA desktop which has built-in applications.

## Features

- Legacy BIOS boot sector with CHS loading plus EDD/CHS runtime disk access
- 32-bit protected-mode C kernel
The ARP, DHCP, DNS, ICMP, UDP and TCP functions are all supported by the IPv4 stack in lwIP 2.2.1.
- PCI Intel e1000 Ethernet driver for 82540EM/82543/82544-compatible adapters
- Network status app, HTTP browser, and asynchronous terminal ping
- First-run account setup followed by mandatory graphical sign-in, with persistent SHA-256 password records, per-account salts, role enforcement, lockout protection, and protected built-in accounts
- Administrator-only searchable User Management app for creating, disabling, enabling, and deleting local accounts
- 32-bit ring-3 process boundary with supervisor-only kernel mappings, per-process user code/stack pages, TSS kernel-stack switching, an `int 0x80` syscall gate, and contained user-mode CPU faults
- Welcome app, logout/session reset, persistent onboarding preference, safe PS/2/USB preference with automatic fallback, and a live clock/date/Ethernet system tray
- Editable native 512x384 RGB565 wallpaper stored on the KreativeOS disk, bilinearly presented on the desktop, and a consumer-style desktop shell
- Auto-selected VESA true-color graphics with boot fallback from 1280x1024 through 640x480
- Crisp 1024x768 native desktop with aspect-preserving downscaling on smaller VBE modes
- VBE direct-colour mask handling for correct RGB output across BIOSes and hypervisors
- Packed scanline framebuffer updates to prevent partially drawn frames on slow virtualized VGA paths
- BIOS VBE mode-list enumeration with native 1024x768x32 preference and portable 24-bit fallback
- Track-sized BIOS wallpaper transfers through a low-memory bounce buffer for fast startup and saving
- Direct CHS floppy I/O without unsupported EDD probing
- Runtime VBE switching between 640x480, 800x600, 1024x768, and 1280x1024 from Settings
- Off-screen scene composition, cursor dirty updates, direct full-window dragging, and continuous interpolated painting
- Polled PS/2 input plus PCI-enumerated UHCI USB boot-keyboard and boot-mouse drivers
- Mixed-case text, scale-safe vector icons, accelerated pointer, desktop, K menu, dock, and clock
- Real multi-window focus, stacking, smooth dragging, minimize, maximize, close, stable launch-order task switching, and Task Manager
- Empty-desktop right-click menu with persistent text-file creation and direct Notes editing
- Kreative File System 3 (KFS3): dual checksummed metadata journals, copy-on-write data banks, 48 files, 1 MiB per file, validated recovery, and BIOS/native-controller-backed reads/writes
- Files, Notes, Images, Paint, Terminal, Browser, Network, Calculator, Calendar, Stopwatch, Hex Viewer, Settings, Task Manager, and About
A true system-level view of boot.bin, kernel.bin, and wallpaper.png, including a hex viewer analysis on a sector by sector basis
- Working app controls, terminal commands, calculator operations, appearance themes, font manager (Modern, Legacy, Bold, Soft, Outline), input speed settings, and drawing
- Raw image usable by v86, QEMU, and VirtualBox
- PIT-driven clock polling with stable RTC reads and taskbar-only refreshes
- Expandable two-stage BIOS loader with EDD/LBA hard-disk support, a 500 KiB resident core region, runtime BSS at 32 MiB, and room for high-memory native components
- A compact compatibility image of 1.44 MiB and a 64 MiB image customized for the modern native edition
- Defensive terminal, editor, paint-palette, calculator, and app-dispatch bounds

## Build

On Windows (NASM and a GCC with 32-bit support on `PATH`):

```powershell
.\build.ps1
```

On Linux or WSL with `nasm`, `gcc-multilib`, binutils, and `make`:

```sh
make
```

The result in the form of a bootable file is build/kreativeos.img and the build is intentionally carried out.
No emulator starts or runs any tests.

On first boot, Set up KreativeOS asks for separate administrator and everyday
The page which is used for logging in neither displays passwords nor contains them as hints.
A screen is offered, and additional accounts can be established at a later date using User Management.

## Run

QEMU (64 MiB minimum; 256 MiB recommended):

```sh
qemu-system-i386 -m 256 -nic user,model=e1000 -drive if=floppy,format=raw,file=build/kreativeos.img
```

VirtualBox: use 256 MiB RAM, 32 MiB video RAM, legacy BIOS, and an Intel
Connect the PRO/1000 MT Desktop (82540EM) adapter using NAT or bridge networking.
Prepare the file named build/kreativeos.img in floppy image format; in the case of v86, use copy.sh to carry out the copying.
the image into a v86 checkout, or select the raw image as the floppy image in
the v86 UI.

## Controls

Complete the first-run setup and then log in; the login screen will not be able to be closed or avoided. You can launch the program by single-clicking on its desktop icon, dock icon, taskbar item, or entry in the K menu.
Or you can select it. To drag a window, grab it by its title bar; the unified title bar should be used.
The desktop can be minimized, maximized or restored, or it can be closed by right-clicking on an empty part of the desktop.
space to create a text file, open Terminal, refresh, or open Appearance
The files created will show up on the desktop and in Files and can then be edited.
The Files section of the Notes has buttons for New, Open, Delete, and paging. Notes Save and
Saving using Ctrl+S writes to the disk and both Paint and Images produce continuous strokes without using the full screen.
Redraws: the Images programme starts by editing the wallpaper used on the desktop and then saves it.
Return to the boot image since the calculator performs guarded integer operations and the calendar
reads the RTC, Stopwatch uses the PIT time base, Hex Viewer reads real disk
You are able to switch to or end any application that is running using the sectors or the Task Manager. Terminal file
The commands are ls, cat, touch, and rm; the network commands are network,
The system must carry out a browser operation, browse the page located at http://host/path, ping the host, and at the same time perform a pingstat.
The commands are sysls, hex filename, and sysrm --force filename. Only an
There is the possibility within an administrator session of running the command `sysrm` or of accessing User Management. `whoami`,
Users are welcome and the logout function allows session operations to be carried out from Terminal.
`isotest` runs an unprivileged ring-3 program that exits through the syscall
The gate causes `crashtest` to generate an invalid-opcode fault in ring 3; the
The kernel terminates that process and then returns control to Terminal.

Calculator arithmetic and Stopwatch time calculations run through ring-3
The notes and the wallpaper editor check the application data for accuracy in an
A worker who is working alone will carry out the action before storing it. A fault in the Calculator's CPU occurs
The calculator can keep on working while the desktop and the other applications are still running. Task
The manager calls these protected execution paths 'Ring 3' or 'Protected I/O'.
The ring-3 worker is employed in date calculations as well. Browser response bodies
and Hex Viewer disk buffers are copied into the bounded user-data region and
had been validated earlier on before the kernel-owned UI renderers used them.

The browser performs an actual DNS lookup, sends real TCP HTTP requests, and then displays the result in a readable format.
The text comes from HTTP pages; support for HTTPS/TLS and Wi-Fi radios is not currently available;
It is necessary to have a wired Intel e1000-family PCI adapter that is compatible.

In the files the system files are read-only and the only method of removing them is to use the explicit sysrm --force command.
Take the boot.bin, kernel.bin, or wallpaper.png files out of the KreativeOS guest image.
After the system has been restarted, the guest image will become unbootable if boot.bin or kernel.bin has been erased;
Its aim is not the files on the host computer.

The notes specifically mention unsaved edits and state that when a save occurs it commits the data to the KFS3 sectors.
The boot image and the saved files are remounted once the guest is rebooted in v86,
guest reboots retain those writes for the running emulator session; export the
modified disk image or emulator state if you also want them after reloading the
browser page.
