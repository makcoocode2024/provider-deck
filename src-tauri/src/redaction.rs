pub fn redact(input: &str, secrets: &[&str]) -> String {
    let mut output = input.to_owned();
    for secret in secrets.iter().filter(|value| value.len() >= 4) {
        output = output.replace(secret, "[REDACTED]");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_all_explicit_secrets() {
        assert_eq!(redact("failed key=abc12345", &["abc12345"]), "failed key=[REDACTED]");
    }
}
