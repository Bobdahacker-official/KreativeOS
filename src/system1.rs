//! Built-in user-mode maintenance worker and its administrator-delegated console.
use crate::{hardware, memory, ring3, storage};
use core::fmt::Write;

struct Service {
    page: [u8; 32],
    next: u64,
    completed: u64,
    scrub: Option<u32>,
    status: &'static str,
}

static mut SERVICE: Service = Service {
    page: [
        2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ],
    next: 0,
    completed: 0,
    scrub: None,
    status: "Waiting for the maintenance worker",
};

pub fn tick(now: u64) {
    // The desktop event loop owns this service and serializes storage access.
    let service = unsafe { &mut SERVICE };
    if now < service.next {
        return;
    }
    service.next = now + 1_000;
    service.page[8..16].copy_from_slice(&(memory::usage().0 as u64).to_le_bytes());
    match ring3::run_maintenance(&mut service.page) {
        Ok(()) => {
            if service.completed == 0 {
                unsafe { hardware::log(b"KreativeOS: system1 ring-3 worker ready\r\n") };
                service.status = "Worker ready";
            }
            service.completed += 1;
        }
        Err(error) => {
            service.status = error;
            return;
        }
    }
    if let Some(cursor) = &mut service.scrub {
        match storage::scrub_free(cursor, service.page[24]) {
            Ok(false) => service.status = "Scrubbing free exFAT clusters",
            Ok(true) => {
                service.scrub = None;
                service.status = "Free-cluster scrub complete";
                unsafe { hardware::log(b"KreativeOS: system1 free-space scrub completed\r\n") };
            }
            Err(error) => {
                service.scrub = None;
                service.status = error;
            }
        }
    }
}

pub fn status(out: &mut impl Write) {
    let service = unsafe { &SERVICE };
    let peak = u64::from_le_bytes(service.page[16..24].try_into().unwrap());
    let _ = writeln!(
        out,
        "{}\nWorker runs: {}  /  Peak heap: {} KiB",
        service.status,
        service.completed,
        peak / 1024
    );
    let _ = writeln!(out, "Scrub budget: {} clusters per run", service.page[0]);
    if let Some(cursor) = service.scrub {
        let _ = writeln!(out, "Next cluster: {cursor}");
    }
}

pub fn command(verb: &str, arg: &str, out: &mut impl Write) {
    if (verb, arg) == ("service", "") {
        status(out);
        return;
    }
    let service = unsafe { &mut SERVICE };
    match (verb, arg) {
        ("scrub", "free") if service.scrub.is_none() => {
            if storage::can_write() && storage::has_exfat() {
                service.scrub = Some(2);
                service.status = "Free-cluster scrub queued";
                let _ = writeln!(out, "Free-cluster scrub queued");
            } else {
                let _ = writeln!(out, "A writable exFAT data volume is required");
            }
        }
        ("scrub", "free") => {
            let _ = writeln!(out, "A scrub is already running");
        }
        ("scrub", "cancel") => {
            service.scrub = None;
            service.status = "Free-cluster scrub cancelled";
            let _ = writeln!(out, "Free-cluster scrub cancelled");
        }
        ("memory", arg) => {
            if let Some(value) = arg
                .strip_prefix("set ")
                .and_then(|value| value.parse::<u8>().ok())
                .filter(|value| (1..=8).contains(value))
            {
                service.page[0] = value;
                let _ = writeln!(
                    out,
                    "Updated the live maintenance policy to {value} clusters per run"
                );
            } else {
                let _ = writeln!(out, "memory set N (1–8 clusters per worker run)");
            }
        }
        _ => {
            let _ = writeln!(out, "service | scrub free | scrub cancel | memory set N");
        }
    }
}
