//! Values a prompt hands to a script: `[name=value]` markers in the prompt text become variables
//! (`${name}`) that `mcp` steps use in tool names, so a test can name the servers it registered.

use std::collections::HashMap;

/// The variables of the markers `[name=value]` in `prompt`: `name` of letters, digits, `_` and
/// `-`, `value` without whitespace and brackets.
pub fn prompt_vars(prompt: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    let mut rest = prompt;
    while let Some(open) = rest.find('[') {
        let after = rest.get(open + 1..).unwrap_or_default();
        let Some(close) = after.find(']') else {
            break;
        };
        let marker = after.get(..close).unwrap_or_default();
        if let Some((name, value)) = marker.split_once('=')
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && !value.is_empty()
            && !value.contains(char::is_whitespace)
        {
            vars.insert(name.to_owned(), value.to_owned());
        }
        rest = after.get(close + 1..).unwrap_or_default();
    }
    vars
}

/// `text` with every `${name}` of `vars` replaced; unknown names stay.
pub fn expand<S: std::hash::BuildHasher>(text: &str, vars: &HashMap<String, String, S>) -> String {
    vars.iter()
        .fold(text.to_owned(), |expanded, (name, value)| {
            expanded.replace(&format!("${{{name}}}"), value)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_in_the_prompt_become_variables_that_expand_in_tool_names() {
        let vars = prompt_vars(
            "[mcp] [docs=mcpd-1a2b3c4d] Use them. [other=mcpo-1a2b3c4d] [bad name=x] [=y] [z=]",
        );

        assert_eq!(vars.len(), 2);
        assert_eq!(expand("${docs}__whoami", &vars), "mcpd-1a2b3c4d__whoami");
        assert_eq!(
            expand("${other}__whoami ${unknown}", &vars),
            "mcpo-1a2b3c4d__whoami ${unknown}"
        );
    }

    #[test]
    fn an_unclosed_bracket_ends_the_scan() {
        assert!(prompt_vars("[a=b").is_empty());
    }
}
