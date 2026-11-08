use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header};
use rsa::pkcs8::{
    DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding,
};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum SigningAlgorithm {
    RS256,
    ES256,
    PS256,
}

impl SigningAlgorithm {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "RS256" => Ok(Self::RS256),
            "ES256" => Ok(Self::ES256),
            "PS256" => Ok(Self::PS256),
            _ => Err(format!("不支持的签名算法: {}", s)),
        }
    }

    pub fn algorithm(self) -> Algorithm {
        self.to_algorithm()
    }

    pub fn to_algorithm(self) -> Algorithm {
        match self {
            Self::RS256 => Algorithm::RS256,
            Self::ES256 => Algorithm::ES256,
            Self::PS256 => Algorithm::PS256,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RS256 => "RS256",
            Self::ES256 => "ES256",
            Self::PS256 => "PS256",
        }
    }
}

#[derive(Clone)]
pub struct KeyManager {
    encoding_key: EncodingKey,
    #[allow(dead_code)]
    decoding_key: DecodingKey,
    algorithm: SigningAlgorithm,
    kid: String,
    public_key_pem: String,
}

impl KeyManager {
    pub fn load_or_generate(
        key_path: &str,
        algorithm: SigningAlgorithm,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = Path::new(key_path);

        if path.exists() {
            tracing::info!("加载现有密钥: {}", key_path);
            Self::load_from_file(key_path, algorithm)
        } else {
            tracing::warn!("密钥文件不存在，正在生成新密钥...");
            let key_manager = Self::generate_new(key_path, algorithm)?;

            tracing::warn!("========================================");
            tracing::warn!("新密钥已生成并保存到: {}", key_path);
            tracing::warn!("签名算法: {}", algorithm.as_str());
            tracing::warn!("密钥 ID (kid): {}", key_manager.kid);
            tracing::warn!("========================================");
            tracing::warn!("重要提示：");
            tracing::warn!("1. 请立即备份密钥文件到安全位置");
            tracing::warn!("2. 密钥丢失会导致已签发的 ID Token 无法验证");
            tracing::warn!("3. 建议设置文件权限为 600 (仅所有者可读写)");
            tracing::warn!("========================================");

            Ok(key_manager)
        }
    }

    fn generate_new(
        key_path: &str,
        algorithm: SigningAlgorithm,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = Path::new(key_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        match algorithm {
            SigningAlgorithm::RS256 | SigningAlgorithm::PS256 => {
                Self::generate_rsa(key_path, algorithm)
            }
            SigningAlgorithm::ES256 => Self::generate_ecdsa(key_path),
        }
    }

    fn generate_rsa(
        key_path: &str,
        algorithm: SigningAlgorithm,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use rsa::rand_core::OsRng;

        let mut rng = OsRng;
        let bits = 2048;
        let private_key = RsaPrivateKey::new(&mut rng, bits)?;
        let public_key = RsaPublicKey::from(&private_key);

        let private_pem = private_key.to_pkcs8_pem(LineEnding::LF)?;
        let public_pem = public_key.to_public_key_pem(LineEnding::LF)?;

        fs::write(key_path, private_pem.as_bytes())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(key_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(key_path, perms)?;
        }

        let kid = Self::compute_kid(public_pem.as_bytes());
        let encoding_key = EncodingKey::from_rsa_pem(private_pem.as_bytes())?;
        let decoding_key = DecodingKey::from_rsa_pem(public_pem.as_bytes())?;

        Ok(Self {
            encoding_key,
            decoding_key,
            algorithm,
            kid,
            public_key_pem: public_pem,
        })
    }

    fn generate_ecdsa(key_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        use p256::ecdsa::SigningKey;
        use p256::elliptic_curve::rand_core::OsRng;
        use p256::pkcs8::{EncodePrivateKey as _, EncodePublicKey as _};

        let mut rng = OsRng;
        let signing_key = SigningKey::random(&mut rng);
        let verifying_key = signing_key.verifying_key();

        let private_pem = signing_key.to_pkcs8_pem(LineEnding::LF)?;
        let public_pem = verifying_key.to_public_key_pem(LineEnding::LF)?;

        fs::write(key_path, private_pem.as_bytes())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(key_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(key_path, perms)?;
        }

        let kid = Self::compute_kid(public_pem.as_bytes());
        let encoding_key = EncodingKey::from_ec_pem(private_pem.as_bytes())?;
        let decoding_key = DecodingKey::from_ec_pem(public_pem.as_bytes())?;

        Ok(Self {
            encoding_key,
            decoding_key,
            algorithm: SigningAlgorithm::ES256,
            kid,
            public_key_pem: public_pem,
        })
    }

    fn load_from_file(
        key_path: &str,
        algorithm: SigningAlgorithm,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let private_pem = fs::read_to_string(key_path)?;

        match algorithm {
            SigningAlgorithm::RS256 | SigningAlgorithm::PS256 => {
                let private_key = RsaPrivateKey::from_pkcs8_pem(&private_pem)?;
                let public_key = RsaPublicKey::from(&private_key);
                let public_pem = public_key.to_public_key_pem(LineEnding::LF)?;

                let kid = Self::compute_kid(public_pem.as_bytes());
                let encoding_key = EncodingKey::from_rsa_pem(private_pem.as_bytes())?;
                let decoding_key = DecodingKey::from_rsa_pem(public_pem.as_bytes())?;

                Ok(Self {
                    encoding_key,
                    decoding_key,
                    algorithm,
                    kid,
                    public_key_pem: public_pem,
                })
            }
            SigningAlgorithm::ES256 => {
                use p256::ecdsa::SigningKey;
                use p256::pkcs8::{DecodePrivateKey as _, EncodePublicKey as _};

                let signing_key = SigningKey::from_pkcs8_pem(&private_pem)?;
                let verifying_key = signing_key.verifying_key();
                let public_pem = verifying_key.to_public_key_pem(LineEnding::LF)?;

                let kid = Self::compute_kid(public_pem.as_bytes());
                let encoding_key = EncodingKey::from_ec_pem(private_pem.as_bytes())?;
                let decoding_key = DecodingKey::from_ec_pem(public_pem.as_bytes())?;

                Ok(Self {
                    encoding_key,
                    decoding_key,
                    algorithm: SigningAlgorithm::ES256,
                    kid,
                    public_key_pem: public_pem,
                })
            }
        }
    }

    fn compute_kid(public_key_pem: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(public_key_pem);
        let hash = hasher.finalize();
        hex::encode(&hash[..8])
    }

    pub fn encoding_key(&self) -> &EncodingKey {
        &self.encoding_key
    }

    pub fn algorithm(&self) -> Algorithm {
        self.algorithm.to_algorithm()
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn header(&self) -> Header {
        let mut header = Header::new(self.algorithm());
        header.kid = Some(self.kid.clone());
        header
    }

    pub fn export_jwk(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        match self.algorithm {
            SigningAlgorithm::RS256 | SigningAlgorithm::PS256 => self.export_rsa_jwk(),
            SigningAlgorithm::ES256 => self.export_ecdsa_jwk(),
        }
    }

    fn export_rsa_jwk(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        use rsa::traits::PublicKeyParts;

        let public_key = RsaPublicKey::from_public_key_pem(&self.public_key_pem)?;
        let n = public_key.n().to_bytes_be();
        let e = public_key.e().to_bytes_be();

        Ok(serde_json::json!({
            "kty": "RSA",
            "use": "sig",
            "kid": self.kid,
            "alg": self.algorithm.as_str(),
            "n": URL_SAFE_NO_PAD.encode(&n),
            "e": URL_SAFE_NO_PAD.encode(&e),
        }))
    }

    fn export_ecdsa_jwk(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        use p256::PublicKey;
        use p256::elliptic_curve::sec1::ToEncodedPoint;
        use p256::pkcs8::DecodePublicKey as _;

        let public_key = PublicKey::from_public_key_pem(&self.public_key_pem)?;
        let point = public_key.to_encoded_point(false);

        let x = point.x().ok_or("无法获取 x 坐标")?;
        let y = point.y().ok_or("无法获取 y 坐标")?;

        Ok(serde_json::json!({
            "kty": "EC",
            "use": "sig",
            "kid": self.kid,
            "alg": "ES256",
            "crv": "P-256",
            "x": URL_SAFE_NO_PAD.encode(x),
            "y": URL_SAFE_NO_PAD.encode(y),
        }))
    }
}
