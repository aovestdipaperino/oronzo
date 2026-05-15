#[derive(Debug, PartialEq)]
pub struct Args {
    pub query: Option<String>,
    pub tools: bool,
    pub thinking: bool,
    pub sidechains: bool,
    pub images: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            query: None,
            tools: true,
            thinking: true,
            sidechains: true,
            images: true,
        }
    }
}

pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut query_parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--no-tools" => out.tools = false,
            "--no-thinking" => out.thinking = false,
            "--no-sidechains" => out.sidechains = false,
            "--no-images" => out.images = false,
            "-h" | "--help" => return Err("__help__".into()),
            s if s.starts_with("--") => return Err(format!("unknown flag: {s}")),
            _ => query_parts.push(a.clone()),
        }
        i += 1;
    }
    if !query_parts.is_empty() {
        out.query = Some(query_parts.join(" "));
    }
    Ok(out)
}

pub fn run(_args: &[String]) {
    eprintln!("mdexport: not yet implemented");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_args_yields_defaults() {
        let a = parse_args(&argv(&[])).unwrap();
        assert_eq!(a, Args::default());
        assert!(a.query.is_none());
        assert!(a.tools && a.thinking && a.sidechains && a.images);
    }

    #[test]
    fn negation_flags_flip_each_class() {
        let a = parse_args(&argv(&["--no-tools", "--no-thinking", "--no-sidechains", "--no-images"])).unwrap();
        assert!(!a.tools);
        assert!(!a.thinking);
        assert!(!a.sidechains);
        assert!(!a.images);
    }

    #[test]
    fn positional_args_join_into_query() {
        let a = parse_args(&argv(&["fix", "auth", "bug"])).unwrap();
        assert_eq!(a.query.as_deref(), Some("fix auth bug"));
    }

    #[test]
    fn flags_and_query_mix() {
        let a = parse_args(&argv(&["--no-tools", "fix", "bug"])).unwrap();
        assert!(!a.tools);
        assert_eq!(a.query.as_deref(), Some("fix bug"));
    }

    #[test]
    fn help_returns_sentinel_error() {
        let e = parse_args(&argv(&["--help"])).unwrap_err();
        assert_eq!(e, "__help__");
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(parse_args(&argv(&["--bogus"])).is_err());
    }
}
