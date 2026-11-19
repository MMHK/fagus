//! Password protection: the standard security handler of PDF 2.0 with AES-256 (ISO 32000-2,
//! §7.6.4, `/V 5` and `/R 6`).
//!
//! A random file key encrypts every string and stream with AES-256 in CBC mode, each with an
//! initialization vector of its own. The user password (needed to open the file) and the owner
//! password (needed to lift the restrictions) each unlock the file key through the entries `U`
//! and `UE`, `O` and `OE` of the encryption dictionary; `Perms` holds the permissions, encrypted
//! with the file key, so that they cannot be changed unnoticed.
//!
//! The key, salts and initialization vectors come from the operating system, so an encrypted
//! PDF differs from run to run. [`Encryption::seed`] replaces that with a generator from a fixed
//! seed, for tests that compare bytes.

use aes::cipher::block_padding::{NoPadding, Pkcs7};
use aes::cipher::{BlockCipherEncrypt, BlockModeEncrypt, KeyInit, KeyIvInit};
use aes::{Aes128, Aes256};
use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::PdfError;
use crate::parse;

/// Something the reader of an encrypted PDF may do without the owner password.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Permission {
    /// Print, in full quality.
    Print,
    /// Print, but only as a low-resolution image; [`Permission::Print`] allows more.
    PrintLowQuality,
    /// Change the content of the document.
    Modify,
    /// Copy or extract text and images.
    Copy,
    /// Add and change annotations (comments), and fill in form fields.
    Annotate,
    /// Fill in form fields, also without [`Permission::Annotate`].
    FillForms,
    /// Insert, delete and rotate pages, and change bookmarks.
    Assemble,
}

impl Permission {
    /// Every permission, which is what an encrypted PDF allows unless told otherwise.
    pub const ALL: [Self; 7] = [
        Self::Print,
        Self::PrintLowQuality,
        Self::Modify,
        Self::Copy,
        Self::Annotate,
        Self::FillForms,
        Self::Assemble,
    ];

    /// The permission named like `print` or `fill-forms`, as the command line and the worker
    /// protocol name them.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|permission| permission.name() == name)
    }

    /// The name of the permission, like `fill-forms`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::PrintLowQuality => "print-low-quality",
            Self::Modify => "modify",
            Self::Copy => "copy",
            Self::Annotate => "annotate",
            Self::FillForms => "fill-forms",
            Self::Assemble => "assemble",
        }
    }

    /// The bits of `P` (ISO 32000-2, table 22) the permission sets.
    fn bits(self) -> u32 {
        let bit = |number: u32| 1 << (number - 1);
        match self {
            Self::Print => bit(3) | bit(12),
            Self::PrintLowQuality => bit(3),
            Self::Modify => bit(4),
            Self::Copy => bit(5),
            Self::Annotate => bit(6),
            Self::FillForms => bit(9),
            Self::Assemble => bit(11),
        }
    }
}

/// How a PDF is protected with passwords.
///
/// ```
/// use fagus_pdf::{Encryption, Permission};
///
/// let encryption = Encryption::new()
///     .user_password("open sesame")
///     .owner_password("change me")
///     .permissions([Permission::Print, Permission::Copy]);
/// ```
#[derive(Clone, Default)]
pub struct Encryption {
    user_password: Option<String>,
    owner_password: Option<String>,
    /// `None` for every permission.
    permissions: Option<Vec<Permission>>,
    seed: Option<[u8; 32]>,
}

impl std::fmt::Debug for Encryption {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The passwords stay out of logs.
        let hidden = |password: &Option<String>| password.as_ref().map(|_| "…");
        formatter
            .debug_struct("Encryption")
            .field("user_password", &hidden(&self.user_password))
            .field("owner_password", &hidden(&self.owner_password))
            .field("permissions", &self.permissions)
            .field("seed", &self.seed.map(|_| "…"))
            .finish()
    }
}

impl Encryption {
    /// Encryption without passwords yet, allowing everything; it needs a user or an owner
    /// password.
    pub fn new() -> Self {
        Self::default()
    }

    /// The password needed to open the PDF. Without one (or with an empty one), the PDF opens
    /// without a password, with only the permissions given.
    #[must_use]
    pub fn user_password(mut self, password: impl Into<String>) -> Self {
        self.user_password = Some(password.into());
        self
    }

    /// The password that lifts the restrictions of the permissions. Without one (or with an
    /// empty one), a random password is used, which nobody knows: the restrictions stay.
    #[must_use]
    pub fn owner_password(mut self, password: impl Into<String>) -> Self {
        self.owner_password = Some(password.into());
        self
    }

    /// What the reader may do without the owner password; everything by default. Viewers
    /// enforce the permissions, the encryption does not: whoever can open the file can read
    /// its content.
    #[must_use]
    pub fn permissions(mut self, permissions: impl IntoIterator<Item = Permission>) -> Self {
        self.permissions = Some(permissions.into_iter().collect());
        self
    }

    /// Takes the key, salts and initialization vectors from a generator with this seed instead
    /// of the operating system, so that the same document gives the same bytes. Only for tests:
    /// whoever knows the seed can decrypt the PDF without a password.
    #[must_use]
    pub fn seed(mut self, seed: [u8; 32]) -> Self {
        self.seed = Some(seed);
        self
    }

    /// The value of `P`: the permissions, with the bits that must be set (7, 8, 13 to 32) and
    /// bit 10, the extraction for accessibility, which PDF 2.0 says to set always.
    fn p(&self) -> u32 {
        let reserved = 0xFFFF_F0C0;
        let accessibility = 1 << 9;
        let granted = self
            .permissions
            .as_deref()
            .unwrap_or(&Permission::ALL)
            .iter()
            .fold(0, |bits, permission| bits | permission.bits());
        reserved | accessibility | granted
    }
}

/// Where the random bytes come from.
enum Random {
    System,
    /// SHA-256 of the seed and a counter.
    Seeded {
        seed: [u8; 32],
        counter: u64,
    },
}

impl Random {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), PdfError> {
        match self {
            Self::System => getrandom::fill(out).map_err(|error| {
                PdfError::Write(format!(
                    "the operating system gave no random numbers for encryption: {error}"
                ))
            }),
            Self::Seeded { seed, counter } => {
                for chunk in out.chunks_mut(32) {
                    let block = Sha256::new()
                        .chain_update(*seed)
                        .chain_update(counter.to_le_bytes())
                        .finalize();
                    *counter += 1;
                    chunk.copy_from_slice(&block[..chunk.len()]);
                }
                Ok(())
            }
        }
    }

    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], PdfError> {
        let mut bytes = [0; N];
        self.fill(&mut bytes)?;
        Ok(bytes)
    }
}

/// Encrypts the strings and streams of one PDF.
pub(crate) struct Encryptor {
    key: [u8; 32],
    random: Random,
    /// The encryption dictionary, `<<` to `>>`.
    dictionary: String,
    /// Why encrypting failed, if it did.
    error: Option<PdfError>,
}

impl Encryptor {
    pub(crate) fn new(encryption: &Encryption) -> Result<Self, PdfError> {
        let user = encryption.user_password.as_deref().unwrap_or("");
        let owner = encryption
            .owner_password
            .as_deref()
            .filter(|password| !password.is_empty());
        if user.is_empty() && owner.is_none() {
            return Err(PdfError::Write(
                "encryption needs a user password or an owner password".to_owned(),
            ));
        }
        let mut random = match encryption.seed {
            Some(seed) => Random::Seeded { seed, counter: 0 },
            None => Random::System,
        };
        let key: [u8; 32] = random.bytes()?;
        // Without an owner password, one nobody knows.
        let random_owner: [u8; 32] = random.bytes()?;
        let owner = owner.map_or(&random_owner[..], str::as_bytes);
        let user = password(user.as_bytes());
        let owner = password(owner);

        // Algorithm 8: U and UE.
        let user_validation: [u8; 8] = random.bytes()?;
        let user_key_salt: [u8; 8] = random.bytes()?;
        let mut u = hash(user, &user_validation, &[]).to_vec();
        u.extend_from_slice(&user_validation);
        u.extend_from_slice(&user_key_salt);
        let ue = wrap(&hash(user, &user_key_salt, &[]), &key);

        // Algorithm 9: O and OE.
        let owner_validation: [u8; 8] = random.bytes()?;
        let owner_key_salt: [u8; 8] = random.bytes()?;
        let mut o = hash(owner, &owner_validation, &u).to_vec();
        o.extend_from_slice(&owner_validation);
        o.extend_from_slice(&owner_key_salt);
        let oe = wrap(&hash(owner, &owner_key_salt, &u), &key);

        // Algorithm 10: Perms.
        let p = encryption.p();
        let mut perms = [0xFF_u8; 16];
        perms[..4].copy_from_slice(&p.to_le_bytes());
        perms[8..12].copy_from_slice(b"Tadb");
        perms[12..].copy_from_slice(&random.bytes::<4>()?);
        let mut block = perms.into();
        Aes256::new(&key.into()).encrypt_block(&mut block);
        let perms: [u8; 16] = block.into();

        let dictionary = format!(
            "<</Filter/Standard/V 5/R 6/Length 256/CF<</StdCF<</AuthEvent/DocOpen/CFM/AESV3/Length 32>>>>\
             /StmF/StdCF/StrF/StdCF/O<{}>/U<{}>/OE<{}>/UE<{}>/P {}/Perms<{}>/EncryptMetadata true>>",
            hex(&o),
            hex(&u),
            hex(&oe),
            hex(&ue),
            p.cast_signed(),
            hex(&perms),
        );
        Ok(Self {
            key,
            random,
            dictionary,
            error: None,
        })
    }

    /// The encryption dictionary, which itself is not encrypted.
    pub(crate) fn dictionary(&self) -> &str {
        &self.dictionary
    }

    /// Why encrypting failed, after a step returned `None`.
    pub(crate) fn take_error(&mut self) -> Option<PdfError> {
        self.error.take()
    }

    /// The data encrypted with AES-256 in CBC mode, after a random initialization vector, padded
    /// as PKCS #7 says (ISO 32000-2, §7.6.3); `None` if there are no random numbers.
    pub(crate) fn encrypt(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        let iv: [u8; 16] = match self.random.bytes() {
            Ok(iv) => iv,
            Err(error) => {
                self.error = Some(error);
                return None;
            }
        };
        let encrypted =
            cbc::Encryptor::<Aes256>::new(&self.key.into(), &iv.into()).encrypt_padded_vec::<Pkcs7>(data);
        let mut out = Vec::with_capacity(16 + encrypted.len());
        out.extend_from_slice(&iv);
        out.extend_from_slice(&encrypted);
        Some(out)
    }

    /// A direct object (such as the dictionary of a stream) with its strings encrypted, each
    /// written as a hexadecimal string; `None` if a string does not end.
    pub(crate) fn encrypt_strings(&mut self, object: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(object.len());
        let mut position = 0;
        while let Some(&byte) = object.get(position) {
            match byte {
                b'(' => {
                    let end = parse::string_end(object, position)?;
                    let encrypted = self.encrypt(&literal(&object[position + 1..end]))?;
                    hex_string(&mut out, &encrypted);
                    position = end + 1;
                }
                b'<' if object.get(position + 1) == Some(&b'<') => {
                    out.extend_from_slice(b"<<");
                    position += 2;
                }
                b'<' => {
                    let end = position + parse::find(&object[position..], b">")?;
                    let encrypted = self.encrypt(&unhex(&object[position + 1..end]))?;
                    hex_string(&mut out, &encrypted);
                    position = end + 1;
                }
                b'%' => {
                    while let Some(&byte) = object.get(position) {
                        if matches!(byte, b'\r' | b'\n') {
                            break;
                        }
                        out.push(byte);
                        position += 1;
                    }
                }
                _ => {
                    out.push(byte);
                    position += 1;
                }
            }
        }
        Some(out)
    }
}

/// A password as the security handler takes it: its UTF-8 bytes, at most 127 of them.
fn password(password: &[u8]) -> &[u8] {
    &password[..password.len().min(127)]
}

/// Algorithm 2.B of ISO 32000-2: the hash of a password with a salt (and, for the owner
/// password, the 48 bytes of `U`).
fn hash(password: &[u8], salt: &[u8; 8], user_key: &[u8]) -> [u8; 32] {
    let mut k: Vec<u8> = Sha256::new()
        .chain_update(password)
        .chain_update(salt)
        .chain_update(user_key)
        .finalize()
        .to_vec();
    let mut round = 0_u32;
    loop {
        let mut k1 = Vec::with_capacity(64 * (password.len() + k.len() + user_key.len()));
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(user_key);
        }
        // 64 repetitions make a multiple of the block size: no padding.
        let e = cbc::Encryptor::<Aes128>::new_from_slices(&k[..16], &k[16..32])
            .expect("a key and an initialization vector of 16 bytes")
            .encrypt_padded_vec::<NoPadding>(&k1);
        // The first 16 bytes as a number modulo 3: 256 is 1 modulo 3, so the sum of the bytes
        // has the same remainder.
        let remainder = e[..16].iter().map(|&byte| u32::from(byte)).sum::<u32>() % 3;
        k = match remainder {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && u32::from(*e.last().expect("not empty")) <= round - 32 {
            break;
        }
    }
    k[..32].try_into().expect("32 bytes")
}

/// The file key encrypted with an intermediate key, for `UE` and `OE`: AES-256 in CBC mode
/// without padding and with an initialization vector of zeros.
fn wrap(intermediate: &[u8; 32], key: &[u8; 32]) -> Vec<u8> {
    cbc::Encryptor::<Aes256>::new(&(*intermediate).into(), &[0; 16].into())
        .encrypt_padded_vec::<NoPadding>(key)
}

/// The bytes of a literal string, between its parentheses (ISO 32000-2, §7.3.4.2).
fn literal(string: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(string.len());
    let mut position = 0;
    while let Some(&byte) = string.get(position) {
        position += 1;
        match byte {
            b'\\' => {
                let Some(&escaped) = string.get(position) else {
                    break;
                };
                position += 1;
                match escaped {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'0'..=b'7' => {
                        let mut value = u32::from(escaped - b'0');
                        for _ in 0..2 {
                            match string.get(position) {
                                Some(&digit @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(digit - b'0');
                                    position += 1;
                                }
                                _ => break,
                            }
                        }
                        // High-order overflow is ignored.
                        out.push(value as u8);
                    }
                    // A backslash at the end of a line continues the string on the next.
                    b'\r' => {
                        if string.get(position) == Some(&b'\n') {
                            position += 1;
                        }
                    }
                    b'\n' => {}
                    other => out.push(other),
                }
            }
            // An end of line in the string is a line feed, however it is written.
            b'\r' => {
                if string.get(position) == Some(&b'\n') {
                    position += 1;
                }
                out.push(b'\n');
            }
            other => out.push(other),
        }
    }
    out
}

/// The bytes of a hexadecimal string, between its angle brackets; a missing last digit is 0.
fn unhex(string: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = string
        .iter()
        .filter_map(|&byte| (byte as char).to_digit(16))
        .map(|digit| digit as u8)
        .collect();
    digits
        .chunks(2)
        .map(|pair| pair[0] << 4 | pair.get(1).copied().unwrap_or(0))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn hex_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'<');
    out.extend_from_slice(hex(bytes).as_bytes());
    out.push(b'>');
}

#[cfg(test)]
pub(crate) mod tests {
    use aes::cipher::{BlockCipherDecrypt, BlockModeDecrypt};

    use super::*;

    /// The file key of an encrypted PDF from its encryption dictionary and a password, user or
    /// owner (Algorithm 2.A), with whether it is the owner's; `None` for a wrong password.
    /// Checks `Perms` against `P` too.
    pub(crate) fn file_key(dictionary: &str, password: &str) -> Option<([u8; 32], bool)> {
        let value = |key: &str| {
            unhex(
                parse::entry(dictionary, key)
                    .expect(key)
                    .trim()
                    .trim_matches(['<', '>'])
                    .as_bytes(),
            )
        };
        let (o, u, oe, ue, perms) = (
            value("/O"),
            value("/U"),
            value("/OE"),
            value("/UE"),
            value("/Perms"),
        );
        let password = super::password(password.as_bytes());
        let salt = |bytes: &[u8]| -> [u8; 8] { bytes.try_into().expect("8 bytes") };
        let (intermediate, encrypted, owner) = if hash(password, &salt(&o[32..40]), &u[..48]) == o[..32] {
            (hash(password, &salt(&o[40..48]), &u[..48]), oe, true)
        } else if hash(password, &salt(&u[32..40]), &[]) == u[..32] {
            (hash(password, &salt(&u[40..48]), &[]), ue, false)
        } else {
            return None;
        };
        let key: [u8; 32] = cbc::Decryptor::<Aes256>::new(&intermediate.into(), &[0; 16].into())
            .decrypt_padded_vec::<NoPadding>(&encrypted)
            .expect("32 bytes")
            .try_into()
            .expect("a key");
        let mut block: [u8; 16] = perms.try_into().expect("16 bytes");
        let mut array = block.into();
        Aes256::new(&key.into()).decrypt_block(&mut array);
        block = array.into();
        let p: i32 = parse::entry(dictionary, "/P")
            .expect("P")
            .trim()
            .parse()
            .expect("a number");
        assert_eq!(block[..4], p.to_le_bytes(), "Perms holds P");
        assert_eq!(&block[8..12], b"Tadb");
        Some((key, owner))
    }

    /// The bytes of a literal or hexadecimal string, with its delimiters.
    pub(crate) fn decode(string: &[u8]) -> Vec<u8> {
        match string {
            [b'(', inner @ .., b')'] => literal(inner),
            [b'<', inner @ .., b'>'] => unhex(inner),
            _ => panic!("not a string"),
        }
    }

    /// Data encrypted with the file key, decrypted.
    pub(crate) fn decrypt(key: &[u8; 32], data: &[u8]) -> Vec<u8> {
        cbc::Decryptor::<Aes256>::new(key.into(), data[..16].try_into().expect("an IV"))
            .decrypt_padded_vec::<Pkcs7>(&data[16..])
            .expect("padded")
    }

    /// The encryption dictionary that qpdf 12.4.2 wrote for
    /// `qpdf --encrypt --user-password=user --owner-password=owner --bits=256 --`, and the file
    /// key that `qpdf --show-encryption-key` showed for it.
    #[test]
    fn unlocks_the_key_as_qpdf_does() {
        let dictionary = "<</CF<</StdCF<</AuthEvent/DocOpen/CFM/AESV3/Length 32>>>>/Filter/Standard/Length 256\
            /O<94ddd6d0a5a10aed97f68f8c3c656785e8b9e42dcd7ee99b9e4c095aacb886bc7a7c790908ff84e43321ceb65b1aabbd>\
            /OE<b1eb3c7598422b0dc24daff05c6d8a544a9c9d5ffb8435adbaa48a03bd364c6d>/P -4\
            /Perms<26603a944b3d42818734650496c91460>/R 6/StmF/StdCF/StrF/StdCF\
            /U<e64ef3b53f76d13590eb0ae22920831f4a0c15226828ebec162851f4ff2ff9c14427dd377aa9c5eba2923827ee75349d>\
            /UE<972a6afe4e3dc236bb5b179b57ff16932bdf06846ce6255a8f3a7f13cff4d457>/V 5>>";
        let key: [u8; 32] = unhex(b"2b2e597f904ed597b9190dd975bce47985f488287b1b184987172eb54a911e4e")
            .try_into()
            .expect("32 bytes");
        assert_eq!(file_key(dictionary, "user"), Some((key, false)));
        assert_eq!(file_key(dictionary, "owner"), Some((key, true)));
        assert_eq!(file_key(dictionary, "User"), None);
    }

    #[test]
    fn reads_literal_strings() {
        assert_eq!(literal(b"a\\(b\\)c\\\\"), b"a(b)c\\");
        assert_eq!(literal(b"\\n\\r\\t\\b\\f"), b"\n\r\t\x08\x0c");
        assert_eq!(literal(b"\\101\\0612\\7"), b"A12\x07");
        assert_eq!(literal(b"line\\\r\nnext\r\nend\rx"), b"linenext\nend\nx");
        assert_eq!(literal(b"\\q"), b"q");
        assert_eq!(unhex(b"48 65 6c6C 7"), b"Hell\x70");
    }

    #[test]
    fn sets_the_bits_of_the_permissions() {
        let p = |permissions: &[Permission]| {
            Encryption::new()
                .permissions(permissions.iter().copied())
                .p()
                .cast_signed()
        };
        assert_eq!(Encryption::new().p().cast_signed(), -4, "everything");
        assert_eq!(p(&[]), -3392, "only extraction for accessibility");
        assert_eq!(p(&[Permission::PrintLowQuality]), -3388);
        assert_eq!(p(&[Permission::Print]), -3388 + (1 << 11));
        assert_eq!(
            p(&[Permission::Copy, Permission::FillForms]),
            -3392 + (1 << 4) + (1 << 8)
        );
        for permission in Permission::ALL {
            assert_eq!(Permission::from_name(permission.name()), Some(permission));
        }
        assert_eq!(Permission::from_name("everything"), None);
    }

    #[test]
    fn unlocks_the_key_with_either_password() {
        let encryption = Encryption::new()
            .user_password("Benutzer ä")
            .owner_password("owner")
            .seed([7; 32]);
        let encryptor = Encryptor::new(&encryption).expect("encryptor");
        let dictionary = encryptor.dictionary();
        assert_eq!(file_key(dictionary, "Benutzer ä"), Some((encryptor.key, false)));
        assert_eq!(file_key(dictionary, "owner"), Some((encryptor.key, true)));
        assert_eq!(file_key(dictionary, "wrong"), None);
        assert_eq!(file_key(dictionary, ""), None);
        // The same seed, the same dictionary.
        assert_eq!(
            Encryptor::new(&encryption).expect("encryptor").dictionary(),
            dictionary
        );

        // Without a user password, the empty one opens the file; the owner password is random.
        let encryptor = Encryptor::new(&Encryption::new().owner_password("owner")).expect("encryptor");
        assert_eq!(file_key(encryptor.dictionary(), ""), Some((encryptor.key, false)));
        assert_eq!(
            file_key(encryptor.dictionary(), "owner"),
            Some((encryptor.key, true))
        );
        let encryptor = Encryptor::new(&Encryption::new().user_password("user")).expect("encryptor");
        assert_eq!(
            file_key(encryptor.dictionary(), "user"),
            Some((encryptor.key, false))
        );
        assert_eq!(file_key(encryptor.dictionary(), ""), None);

        assert!(Encryptor::new(&Encryption::new()).is_err());
        assert!(Encryptor::new(&Encryption::new().user_password("").owner_password("")).is_err());
    }

    #[test]
    fn encrypts_strings_and_data() {
        let mut encryptor =
            Encryptor::new(&Encryption::new().user_password("user").seed([1; 32])).expect("encryptor");
        let data = b"BT /F1 12 Tf (Rechnung) Tj ET".repeat(3);
        let encrypted = encryptor.encrypt(&data).expect("encrypted");
        assert_eq!(encrypted.len(), 16 + (data.len() / 16 + 1) * 16);
        assert_eq!(decrypt(&encryptor.key, &encrypted), data);
        // A new initialization vector every time.
        assert_ne!(encryptor.encrypt(&data).expect("encrypted"), encrypted);
        let empty = encryptor.encrypt(b"").expect("encrypted");
        assert_eq!(empty.len(), 32);
        assert_eq!(decrypt(&encryptor.key, &empty), b"");

        let dictionary = b"<</Type/EmbeddedFile/Params<</ModDate(D:2026\\)x)/CheckSum<0102>>>/Length 3>>";
        let encrypted = encryptor.encrypt_strings(dictionary).expect("strings end");
        let text = String::from_utf8(encrypted).expect("text");
        assert!(
            text.starts_with("<</Type/EmbeddedFile/Params<</ModDate<"),
            "{text}"
        );
        let params = parse::entry(&text, "/Params").expect("Params");
        let string = |key| {
            unhex(
                parse::entry(params, key)
                    .expect(key)
                    .trim()
                    .trim_matches(['<', '>'])
                    .as_bytes(),
            )
        };
        assert_eq!(decrypt(&encryptor.key, &string("/ModDate")), b"D:2026)x");
        assert_eq!(decrypt(&encryptor.key, &string("/CheckSum")), [1, 2]);
        assert_eq!(parse::entry(&text, "/Length"), Some(" 3"));
        assert_eq!(encryptor.encrypt_strings(b"<</T(open"), None);
    }

    #[test]
    fn hides_the_passwords() {
        let encryption = Encryption::new().user_password("secret").owner_password("hidden");
        let debug = format!("{encryption:?}");
        assert!(!debug.contains("secret") && !debug.contains("hidden"), "{debug}");
    }
}
