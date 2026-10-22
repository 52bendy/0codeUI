//! 一次性确认令牌的生成与校验。
//!
//! - 令牌由两个 UUID v4（各 122 位随机）拼接，共 244 位熵；
//! - 数据库只保存 SHA-256 摘要，泄露数据库也无法还原令牌；
//! - 校验使用常量时间比较，避免时序侧信道；
//! - 令牌与确认单一一对应，且只能成功消费一次（由仓储层原子状态迁移保证）。

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// 令牌前缀。
const TOKEN_PREFIX: &str = "cct_";

/// 生成新令牌。
pub fn generate() -> String {
    format!(
        "{TOKEN_PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

/// 计算令牌摘要（SHA-256 hex）。
pub fn hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

/// 常量时间校验令牌与摘要是否匹配。
pub fn verify(token: &str, expected_hash: &str) -> bool {
    let actual = hash(token);
    actual.as_bytes().ct_eq(expected_hash.as_bytes()).into()
}

/// 令牌格式是否合法（在查询数据库前做廉价校验，避免无谓 IO）。
pub fn is_well_formed(token: &str) -> bool {
    let Some(rest) = token.strip_prefix(TOKEN_PREFIX) else {
        return false;
    };
    rest.len() == 64 && rest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_unique_and_well_formed() {
        let first = generate();
        let second = generate();
        assert_ne!(first, second);
        assert!(is_well_formed(&first));
        assert!(is_well_formed(&second));
        assert!(!is_well_formed("cct_short"));
        assert!(!is_well_formed("bogus"));
    }

    #[test]
    fn hash_is_stable_and_verification_works() {
        let token = generate();
        let digest = hash(&token);
        assert_eq!(digest.len(), 64);
        assert_eq!(hash(&token), digest);
        assert!(verify(&token, &digest));
        assert!(!verify(&generate(), &digest));
    }

    #[test]
    fn hash_does_not_leak_token() {
        let token = generate();
        assert!(!hash(&token).contains(token.trim_start_matches("cct_")));
    }
}
