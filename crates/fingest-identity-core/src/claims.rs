use serde::{Deserialize, Serialize};

/// JWT payload. Field names and types are byte-identical to v1 so tokens issued by either
/// version remain interchangeable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Subject: the account login.
    pub sub: String,
    pub admin: bool,
    /// Expiry, seconds since the Unix epoch.
    pub exp: i64,
    /// Issued-at, seconds since the Unix epoch.
    pub iat: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_keys_match_v1() {
        let claims = Claims {
            sub: "bob".into(),
            admin: true,
            exp: 1_800_000_000,
            iat: 1_700_000_000,
        };

        let json = serde_json::to_value(&claims).unwrap();
        assert_eq!(json["sub"], "bob");
        assert_eq!(json["admin"], true);
        assert_eq!(json["exp"], 1_800_000_000_i64);
        assert_eq!(json["iat"], 1_700_000_000_i64);
        assert_eq!(json.as_object().unwrap().len(), 4, "no extra claims");
    }
}
