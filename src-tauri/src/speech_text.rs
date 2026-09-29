//! Shared normalization that turns Markdown replies into speakable prose.
//!
//! Both speech providers must read exactly the same text, so the cleanup lives here instead of
//! inside one provider. It strips code fences, Markdown emphasis, links and URLs, then collapses
//! whitespace into a single line that text-to-speech engines can consume directly.

pub fn prepare_for_speech(text: &str) -> String {
    let mut prepared = String::new();
    let mut in_code_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }
        let cleaned_line = strip_markdown_line(line);
        if !cleaned_line.trim().is_empty() {
            append_normalized(&mut prepared, "\n");
            append_normalized(&mut prepared, &cleaned_line);
        }
    }
    prepared.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_markdown_line(line: &str) -> String {
    let characters = line.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while characters
        .get(index)
        .is_some_and(|character| character.is_whitespace())
    {
        index += 1;
    }
    while characters
        .get(index)
        .is_some_and(|character| matches!(character, '#' | '>'))
    {
        index += 1;
    }
    if characters
        .get(index)
        .is_some_and(|character| matches!(character, '-' | '+' | '*'))
        && characters
            .get(index + 1)
            .is_some_and(|character| character.is_whitespace())
    {
        index += 1;
    } else {
        let number_start = index;
        while characters
            .get(index)
            .is_some_and(|character| character.is_ascii_digit())
        {
            index += 1;
        }
        if index > number_start
            && characters.get(index) == Some(&'.')
            && characters
                .get(index + 1)
                .is_some_and(|character| character.is_whitespace())
        {
            index += 1;
        } else {
            index = number_start;
        }
    }
    while characters
        .get(index)
        .is_some_and(|character| character.is_whitespace())
    {
        index += 1;
    }
    while index < characters.len() {
        let character = characters[index];
        let is_image = character == '!' && characters.get(index + 1) == Some(&'[');
        let is_link = character == '[';
        if is_image || is_link {
            let label_start = if is_image { index + 2 } else { index + 1 };
            if let Some(label_end) = characters[label_start..]
                .iter()
                .position(|item| *item == ']')
            {
                let label_end = label_start + label_end;
                if characters.get(label_end + 1) == Some(&'(') {
                    if let Some(url_end) = characters[label_end + 2..]
                        .iter()
                        .position(|item| *item == ')')
                    {
                        let label = characters[label_start..label_end]
                            .iter()
                            .collect::<String>();
                        append_normalized(&mut output, &strip_markdown_line(&label));
                        index = label_end + 3 + url_end;
                        continue;
                    }
                }
            }
        }
        let remaining = characters[index..].iter().collect::<String>();
        if (remaining.starts_with("https://") || remaining.starts_with("http://"))
            && (index == 0 || characters[index - 1].is_whitespace())
        {
            while index < characters.len() && !characters[index].is_whitespace() {
                index += 1;
            }
            continue;
        }
        if character != '`' && is_speech_character(character) {
            output.push(character);
        }
        index += 1;
    }
    output
}

fn append_normalized(target: &mut String, value: &str) {
    for character in value.chars() {
        if character.is_whitespace() {
            if !target.ends_with(' ') && !target.is_empty() {
                target.push(' ');
            }
        } else {
            target.push(character);
        }
    }
}

fn is_speech_character(character: char) -> bool {
    character.is_alphanumeric()
        || character.is_whitespace()
        || matches!(
            character,
            '，' | '。'
                | '！'
                | '？'
                | '；'
                | '：'
                | '、'
                | ','
                | '.'
                | '!'
                | '?'
                | ';'
                | ':'
                | '…'
                | '—'
                | '-'
                | '('
                | ')'
                | '（'
                | '）'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '「'
                | '」'
                | '《'
                | '》'
        )
}

#[cfg(test)]
mod tests {
    use super::prepare_for_speech;

    #[test]
    fn prepares_markdown_without_speaking_symbols_or_urls() {
        let prepared = prepare_for_speech(
            "## **Nova** 🎉\n\n请查看 [官方文档](https://example.com/docs)。\n`代码` 和 @#$%",
        );
        assert_eq!(prepared, "Nova 请查看 官方文档。 代码 和");
    }

    #[test]
    fn removes_markdown_list_prefixes_but_keeps_normal_hyphens() {
        let prepared = prepare_for_speech("- 第一项\n2. 第二项\n> 引用\n温度 -5 度");
        assert_eq!(prepared, "第一项 第二项 引用 温度 -5 度");
    }
}
