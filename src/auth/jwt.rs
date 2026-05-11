use anyhow::Result;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub iat: usize,
}

pub fn create_token(username: &str, secret: &str) -> Result<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as usize;
    let claims = Claims {
        sub: username.to_string(),
        iat: now,
        exp: now + 86400, // 24 hours
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn validate_token(token: &str, secret: &str) -> Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_validate() {
        let secret = "test-secret-key";
        let token = create_token("admin", secret).unwrap();
        let claims = validate_token(&token, secret).unwrap();
        assert_eq!(claims.sub, "admin");
    }

    #[test]
    fn test_invalid_token() {
        let result = validate_token("invalid-token", "secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_secret() {
        let token = create_token("admin", "secret1").unwrap();
        let result = validate_token(&token, "secret2");
        assert!(result.is_err());
    }
}
