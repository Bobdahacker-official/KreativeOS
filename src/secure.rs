//! TLS 1.3 transport. Chain, hostname, validity and handshake signatures are mandatory.
use crate::{hardware, net::Network, text::Text};
use alloc::vec::Vec;
use embedded_tls::blocking::{
    Aes128GcmSha256, CertificateEntryRef, CertificateRef, CertificateVerifyRef, CryptoProvider,
    TlsConfig, TlsConnection, TlsContext, TlsError, TlsVerifier,
};
use rand_core::{CryptoRng, RngCore};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use sha2::{Digest, Sha256};

fn random(bytes: &mut [u8]) -> Result<(), getrandom::Error> {
    if !entropy_available() {
        return Err(getrandom::Error::UNSUPPORTED);
    }
    for chunk in bytes.chunks_mut(8) {
        let mut value = 0u64;
        let mut ok = false;
        for _ in 0..32 {
            if unsafe { core::arch::x86_64::_rdrand64_step(&mut value) } == 1 {
                ok = true;
                break;
            }
        }
        if !ok {
            return Err(getrandom::Error::UNSUPPORTED);
        }
        chunk.copy_from_slice(&value.to_le_bytes()[..chunk.len()]);
    }
    Ok(())
}
getrandom::register_custom_getrandom!(random);
pub fn entropy_available() -> bool {
    core::arch::x86_64::__cpuid(1).ecx & (1 << 30) != 0
}
struct Random;
impl CryptoRng for Random {}
impl RngCore for Random {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0; 4];
        self.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0; 8];
        self.fill_bytes(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, b: &mut [u8]) {
        self.try_fill_bytes(b).expect("Hardware random failed")
    }
    fn try_fill_bytes(&mut self, b: &mut [u8]) -> Result<(), rand_core::Error> {
        random(b).map_err(rand_core::Error::from)
    }
}
struct Verifier {
    host: Text<128>,
    leaf: Vec<u8>,
    hash: [u8; 32],
    authenticated: bool,
}
impl Verifier {
    fn new() -> Self {
        Self {
            host: Text::new(),
            leaf: Vec::new(),
            hash: [0; 32],
            authenticated: false,
        }
    }
}
impl TlsVerifier<Aes128GcmSha256> for Verifier {
    fn set_hostname_verification(&mut self, host: &str) -> Result<(), TlsError> {
        ServerName::try_from(host).map_err(|_| TlsError::InvalidCertificate)?;
        self.host.set(host);
        Ok(())
    }
    fn verify_certificate(
        &mut self,
        transcript: &Sha256,
        cert: CertificateRef,
    ) -> Result<(), TlsError> {
        let leaf = match cert.entries.first() {
            Some(CertificateEntryRef::X509(v)) => *v,
            _ => return Err(TlsError::InvalidCertificate),
        };
        let der = CertificateDer::from(leaf);
        let ee = webpki::EndEntityCert::try_from(&der).map_err(|_| TlsError::InvalidCertificate)?;
        let chain: Vec<_> = cert
            .entries
            .iter()
            .skip(1)
            .filter_map(|e| match e {
                CertificateEntryRef::X509(v) => Some(CertificateDer::from(*v)),
                _ => None,
            })
            .collect();
        let provider = rustls_rustcrypto::provider();
        let now = hardware::unix_time().ok_or(TlsError::InvalidCertificate)?;
        #[cfg(not(feature = "tls-smoke"))]
        let roots = webpki_roots::TLS_SERVER_ROOTS;
        #[cfg(feature = "tls-smoke")]
        let roots = {
            let mut roots = webpki_roots::TLS_SERVER_ROOTS.to_vec();
            let ca = CertificateDer::from(include_bytes!("../build/tls-test/ca.der").as_slice());
            roots.push(
                webpki::anchor_from_trusted_cert(&ca)
                    .map_err(|_| TlsError::InvalidCertificate)?
                    .to_owned(),
            );
            roots
        };

        ee.verify_for_usage(
            provider.signature_verification_algorithms.all,
            &roots,
            &chain,
            UnixTime::since_unix_epoch(core::time::Duration::from_secs(now)),
            webpki::KeyUsage::server_auth(),
            None,
            None,
        )
        .map_err(|e| {
            diagnostic("Certificate", &e);
            TlsError::InvalidCertificate
        })?;
        let host =
            ServerName::try_from(self.host.str()).map_err(|_| TlsError::InvalidCertificate)?;
        ee.verify_is_valid_for_subject_name(&host).map_err(|e| {
            diagnostic("Hostname", &e);
            TlsError::InvalidCertificate
        })?;
        self.leaf = leaf.to_vec();
        self.hash.copy_from_slice(&transcript.clone().finalize());
        Ok(())
    }
    fn verify_signature(&mut self, v: CertificateVerifyRef) -> Result<(), TlsError> {
        let mut msg = Vec::from([32u8; 64]);
        msg.extend_from_slice(b"TLS 1.3, server CertificateVerify\0");
        msg.extend_from_slice(&self.hash);
        let der = CertificateDer::from(self.leaf.as_slice());
        let ee = webpki::EndEntityCert::try_from(&der).map_err(|_| TlsError::InvalidCertificate)?;
        let provider = rustls_rustcrypto::provider();
        let scheme = v.signature_scheme.as_u16();
        // Only TLS 1.3 CertificateVerify signature schemes are permitted.
        if ![0x0403, 0x0503, 0x0804, 0x0805, 0x0806, 0x0807].contains(&scheme) {
            return Err(TlsError::InvalidCertificate);
        }
        for (id, algorithms) in provider.signature_verification_algorithms.mapping {
            if u16::from(*id) == scheme {
                if let Some(alg) = algorithms.first() {
                    if ee.verify_signature(*alg, &msg, v.signature).is_ok() {
                        self.authenticated = true;
                        return Ok(());
                    }
                }
            }
        }
        Err(TlsError::InvalidCertificate)
    }
}
fn diagnostic(label: &str, e: &impl core::fmt::Debug) {
    use core::fmt::Write;
    let mut t: Text<256> = Text::new();
    let _ = write!(t, "TLS {}: {:?}\r\n", label, e);
    unsafe { hardware::log(&t.bytes[..t.len]) }
}
struct Provider {
    rng: Random,
    verifier: Verifier,
}
impl CryptoProvider for Provider {
    type CipherSuite = Aes128GcmSha256;
    type Signature = Vec<u8>;
    fn rng(&mut self) -> impl rand_core::CryptoRngCore {
        &mut self.rng
    }
    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Self::CipherSuite>, TlsError> {
        Ok(&mut self.verifier)
    }
}
type IoError = embedded_io::ErrorKind;
pub struct Socket<'a>(pub &'a mut Network);
impl embedded_io::ErrorType for Socket<'_> {
    type Error = IoError;
}
impl embedded_io::Read for Socket<'_> {
    fn read(&mut self, b: &mut [u8]) -> Result<usize, IoError> {
        if b.is_empty() {
            return Ok(0);
        }
        let start = hardware::millis();
        loop {
            self.0.poll();
            let count = b.len().min(self.0.raw.len());
            if count > 0 {
                for v in &mut b[..count] {
                    *v = self.0.raw.pop_front().unwrap()
                }
                return Ok(count);
            }
            if self.0.raw_eof {
                return Ok(0);
            }
            if self.0.state != 6 || hardware::millis() - start > 15000 {
                return Err(IoError::TimedOut);
            }
            core::hint::spin_loop()
        }
    }
}
impl embedded_io::Write for Socket<'_> {
    fn write(&mut self, b: &[u8]) -> Result<usize, IoError> {
        let n = b.len().min(1200);
        if n == 0 {
            return Ok(0);
        }
        self.0.raw_send(&b[..n]);
        let start = hardware::millis();
        while self.0.raw_waiting {
            self.0.poll();
            if self.0.state != 6 || hardware::millis() - start > 10000 {
                return Err(IoError::TimedOut);
            }
        }
        Ok(n)
    }
    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}
pub fn fetch(n: &mut Network) {
    n.tls_pending = false;
    n.body.clear();
    if !entropy_available() {
        n.tls_result(false, "HTTPS requires a hardware random source");
        return;
    }
    let host = n.host;
    let path = n.path;
    let result = (|| -> Result<Text<12288>, TlsError> {
        let mut read = alloc::vec![0;18432];
        let mut write = alloc::vec![0;4096];
        let config = TlsConfig::new()
            .enable_rsa_signatures()
            .with_server_name(host.str());
        let mut tls: TlsConnection<_, Aes128GcmSha256> =
            TlsConnection::new(Socket(n), &mut read, &mut write);
        let mut provider = Provider {
            rng: Random,
            verifier: Verifier::new(),
        };
        tls.open(TlsContext::new(&config, &mut provider))?;
        if !provider.verifier.authenticated {
            return Err(TlsError::InvalidCertificate);
        }
        unsafe { hardware::log(b"KreativeOS: TLS certificate and hostname verified\r\n") };
        let mut request: Text<512> = Text::new();
        request.append("GET ");
        request.append(path.str());
        request.append(" HTTP/1.0\r\nHost: ");
        request.append(host.str());
        request.append("\r\nUser-Agent: KreativeOS/0.2\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n");
        let mut offset = 0;
        while offset < request.len {
            offset += tls.write(&request.bytes[offset..request.len])?;
        }
        tls.flush()?;
        let mut body: Text<12288> = Text::new();
        let mut buf = [0; 2048];
        loop {
            match tls.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => {
                    for &b in &buf[..count] {
                        body.push(if b < 128 { b } else { b'?' })
                    }
                    if complete(body.str()) || body.len == 12288 {
                        break;
                    }
                }
                Err(e) => {
                    if complete(body.str()) {
                        break;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        let _ = tls.close();
        Ok(body)
    })();
    match result {
        Ok(body) => {
            n.body = body;
            n.tls_result(true, "Secure connection - TLS 1.3");
            unsafe { hardware::log(b"KreativeOS: HTTPS page loaded\r\n") }
        }
        Err(e) => {
            n.cert_rejected = matches!(e, TlsError::InvalidCertificate);
            diagnostic("request", &e);
            n.tls_result(
                false,
                "HTTPS failed: certificate, connection or protocol error",
            )
        }
    }
}
fn complete(s: &str) -> bool {
    let Some((h, b)) = s.split_once("\r\n\r\n") else {
        return false;
    };
    for line in h.split("\r\n") {
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                return value
                    .trim()
                    .parse::<usize>()
                    .map(|n| b.len() >= n)
                    .unwrap_or(false);
            }
        }
    }
    false
}
#[cfg(feature = "tls-smoke")]
pub fn smoke(n: &mut Network) {
    for (url, expected) in [
        ("https://10.0.2.2:8443/", true),
        ("https://10.0.2.2:8444/", false),
        ("https://10.0.2.2:8445/", false),
    ] {
        n.browse(url);
        let start = hardware::millis();
        while n.state != 0 && !n.tls_pending && hardware::millis() - start < 20000 {
            n.poll()
        }
        if n.tls_pending {
            fetch(n)
        }
        let good = n.verified && n.body.str().contains("KreativeOS HTTPS verified");
        if good != expected || (expected && !n.verified) || (!expected && !n.cert_rejected) {
            unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED\r\n") };
            return;
        }
        unsafe {
            hardware::log(if expected {
                b"KreativeOS: HTTPS positive PASS\r\n"
            } else {
                b"KreativeOS: HTTPS rejection PASS\r\n"
            })
        };
    }
}
