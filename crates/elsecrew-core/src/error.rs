use serde::Serialize;

/// 닥터가 초보자용 안내로 바꾸는 에러 분류. 프론트 `doctor.ts`의 분류와 1:1.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    Network,
    Checksum,
    NotFound,
    Permission,
    Disk,
    Generic,
}

/// 프론트로 전달되는 구조화된 에러. `kind`는 닥터가 로컬라이즈된 안내를 고르는 근거,
/// `detail`은 "자세한 내용"에 보여줄 원시 기술 정보(영어, 복사해서 도움 요청용).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub kind: ErrorKind,
    pub detail: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
    pub fn generic(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Generic, detail)
    }
    pub fn network(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Network, detail)
    }
    pub fn checksum(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Checksum, detail)
    }
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, detail)
    }

    /// io/툴 출력 등 원시 텍스트에서 원인을 추정해 분류한다(detail은 그 원문).
    pub fn classify(detail: impl Into<String>) -> Self {
        let d = detail.into();
        Self::new(classify_kind(&d), d)
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::classify(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::classify(s.to_string())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

/// 원시 에러 텍스트를 분류한다(정규식 없이 소문자 부분일치, 의존성 0).
pub fn classify_kind(text: &str) -> ErrorKind {
    let t = text.to_lowercase();
    let has = |pats: &[&str]| pats.iter().any(|p| t.contains(p));
    if has(&[
        "could not connect",
        "failed to connect",
        "connection refused",
        "network",
        "timed out",
        "timeout",
        "resolve host",
        "getaddrinfo",
        "dns",
        "offline",
    ]) {
        ErrorKind::Network
    } else if has(&["checksum", "verification failed", "corrupt"]) {
        ErrorKind::Checksum
    } else if has(&[
        "command not found",
        "not recognized",
        "no such file",
        "enoent",
    ]) {
        ErrorKind::NotFound
    } else if has(&[
        "permission denied",
        "eacces",
        "not permitted",
        "operation not permitted",
    ]) {
        ErrorKind::Permission
    } else if has(&["no space", "enospc", "disk full", "disk is full"]) {
        ErrorKind::Disk
    } else {
        ErrorKind::Generic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_common_errors() {
        assert_eq!(
            classify_kind("curl: (28) Failed to connect to downloads.claude.ai"),
            ErrorKind::Network
        );
        assert_eq!(
            classify_kind("Checksum verification failed"),
            ErrorKind::Checksum
        );
        assert_eq!(
            classify_kind("EACCES: permission denied, open '/usr/local/bin'"),
            ErrorKind::Permission
        );
        assert_eq!(
            classify_kind("ENOSPC: no space left on device"),
            ErrorKind::Disk
        );
        assert_eq!(classify_kind("ENOENT: no such file"), ErrorKind::NotFound);
        assert_eq!(classify_kind("something unexpected"), ErrorKind::Generic);
    }

    #[test]
    fn from_string_classifies() {
        let e: AppError = "connection refused".to_string().into();
        assert_eq!(e.kind, ErrorKind::Network);
    }
}
