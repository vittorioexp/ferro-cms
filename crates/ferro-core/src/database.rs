//! Which SQL database backs this Ferro instance.

use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DatabaseKindError(pub String);

pub type Result<T> = std::result::Result<T, DatabaseKindError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseKind {
    Postgres,
    Sqlite,
    Mysql,
}

impl DatabaseKind {
    /// Infer backend from the URL scheme. If the scheme is unknown, use `override_kind` when set.
    pub fn detect(url: &str, override_kind: Option<&str>) -> Result<Self> {
        match Self::infer_from_url(url) {
            Ok(kind) => Ok(kind),
            Err(e) => match override_kind {
                Some(k) => k.parse(),
                None => Err(e),
            },
        }
    }

    fn infer_from_url(url: &str) -> Result<Self> {
        let lower = url.trim().to_ascii_lowercase();
        if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
            Ok(DatabaseKind::Postgres)
        } else if lower.starts_with("sqlite:") {
            Ok(DatabaseKind::Sqlite)
        } else if lower.starts_with("mysql://") || lower.starts_with("mariadb://") {
            Ok(DatabaseKind::Mysql)
        } else {
            Err(DatabaseKindError(format!(
                "unsupported FERRO_DATABASE_URL scheme (use postgres://, sqlite:, or mysql://, or set FERRO_DATABASE_KIND): {}",
                url.split(':').next().unwrap_or(url)
            )))
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DatabaseKind::Postgres => "postgres",
            DatabaseKind::Sqlite => "sqlite",
            DatabaseKind::Mysql => "mysql",
        }
    }
}

impl FromStr for DatabaseKind {
    type Err = DatabaseKindError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Ok(DatabaseKind::Postgres),
            "sqlite" | "sqlite3" => Ok(DatabaseKind::Sqlite),
            "mysql" | "mariadb" => Ok(DatabaseKind::Mysql),
            other => Err(DatabaseKindError(format!("unknown database kind `{other}`"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_schemes() {
        assert_eq!(
            DatabaseKind::detect("postgres://localhost/x", None).unwrap(),
            DatabaseKind::Postgres
        );
        assert_eq!(DatabaseKind::detect("sqlite://./a.db", None).unwrap(), DatabaseKind::Sqlite);
        assert_eq!(DatabaseKind::detect("sqlite::memory:", None).unwrap(), DatabaseKind::Sqlite);
        assert_eq!(DatabaseKind::detect("mysql://localhost/x", None).unwrap(), DatabaseKind::Mysql);
        assert_eq!(
            DatabaseKind::detect("postgres://localhost/x", Some("sqlite")).unwrap(),
            DatabaseKind::Postgres
        );
        assert_eq!(
            DatabaseKind::detect("postgres://localhost/x", Some("postgres")).unwrap(),
            DatabaseKind::Postgres
        );
        assert_eq!(
            DatabaseKind::detect("unknown://host/db", Some("sqlite")).unwrap(),
            DatabaseKind::Sqlite
        );
        assert!(DatabaseKind::detect("unknown://host/db", None).is_err());
    }
}
