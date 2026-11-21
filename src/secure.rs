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
        if self.0.wifi.active {
            return self.0.wifi.read(b);
        }
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
        if self.0.wifi.active {
            return self.0.wifi.write(b);
        }
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
    let port = n.remote_port;
    let result = (|| -> Result<crate::web::Response, TlsError> {
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
        let request = crate::web::request(host.str(), path.str(), port, true);
        let mut offset = 0;
        while offset < request.len {
            let sent = tls.write(&request.bytes[offset..request.len])?;
            if sent == 0 {
                return Err(TlsError::ConnectionClosed);
            }
            offset += sent;
        }
        tls.flush()?;
        let mut body = Vec::new();
        let mut buf = [0; 4096];
        let start = hardware::millis();
        loop {
            if hardware::millis() - start > 30000 {
                return Err(TlsError::Io(IoError::TimedOut));
            }
            match tls.read(&mut buf) {
                Ok(0) | Err(TlsError::ConnectionClosed) => break,
                Ok(count) => {
                    if body.len() + count > crate::web::MAX_RESPONSE {
                        return Err(TlsError::InsufficientSpace);
                    }
                    body.extend_from_slice(&buf[..count]);
                    if crate::web::complete(&body) {
                        break;
                    }
                }
                Err(e) => return Err(e),
            }
        }
        let _ = tls.close();
        crate::web::parse(body).ok_or(TlsError::DecodeError)
    })();
    match result {
        Ok(response) => {
            n.accept_response(response);
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
#[cfg(feature = "tls-smoke")]
pub fn smoke(n: &mut Network) {
    let mut document = Text::new();
    if !smoke_certificates(n, &mut document) {
        return;
    }
    let base = "https://10.0.2.2:8443/";
    let (image_url, audio_url) = crate::web::media_urls(base, document.str());
    let (Some(image_url), Some(audio_url)) = (image_url, audio_url) else {
        unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED - HTML media links\r\n") };
        return;
    };
    let image = smoke_get(n, image_url.str());
    if image
        .as_ref()
        .and_then(|r| crate::pictures::decode(&r.bytes))
        .is_none()
    {
        unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED - image\r\n") };
        return;
    }
    let clip = smoke_get(n, audio_url.str());
    if !clip
        .as_ref()
        .is_some_and(|r| crate::audio::play(&r.bytes).is_ok())
    {
        unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED - audio\r\n") };
        return;
    }
    unsafe { hardware::log(b"KreativeOS: HTTPS media PASS\r\n") };
    if let Err(error) = n.wifi.join("KreativeOSTest", "test-credentials") {
        diagnostic("Wi-Fi fixture association", &error);
        unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED - Wi-Fi\r\n") };
        return;
    }
    if smoke_get(n, base).is_none() {
        diagnostic("Wi-Fi fixture TCP", &n.status);
        unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED - Wi-Fi\r\n") };
        return;
    }
    unsafe { hardware::log(b"KreativeOS: ESP-AT HTTPS PASS\r\n") };
}

#[cfg(feature = "tls-smoke")]
fn smoke_get(n: &mut Network, url: &str) -> Option<crate::web::Response> {
    n.browse(url);
    let start = hardware::millis();
    while n.state != 0 && !n.tls_pending && hardware::millis() - start < 20000 {
        n.poll();
    }
    if n.tls_pending {
        fetch(n);
    }
    n.response.take()
}

#[cfg(feature = "tls-smoke")]
fn smoke_certificates(n: &mut Network, document: &mut Text<12288>) -> bool {
    let start = "http://10.0.2.2:18080/start?old=1";
    let redirect = smoke_get(n, start)
        .and_then(|response| crate::web::resolve(start, response.location.str()));
    if !redirect.is_some_and(|url| url.str() == "https://10.0.2.2:8443/?view=1") {
        unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED - HTTP redirect\r\n") };
        return false;
    }
    for (url, expected) in [
        ("https://10.0.2.2:8443/?view=1", true),
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
            return false;
        }
        if expected {
            let links = crate::web::links(url, n.body.str());
            if links.len() != 1 || links[0].url.str() != "https://10.0.2.2:8443/?view=2&mode=read" {
                unsafe { hardware::log(b"KreativeOS: TLS CHECK FAILED - page link\r\n") };
                return false;
            }
            *document = n.body;
        }
        unsafe {
            hardware::log(if expected {
                b"KreativeOS: HTTP redirect, HTTPS gzip and page link PASS\r\n"
            } else {
                b"KreativeOS: HTTPS rejection PASS\r\n"
            })
        };
    }
    true
}
