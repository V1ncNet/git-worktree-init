enum Token {
    Star,
    Any,
    Literal(char),
    Set { negated: bool, members: Vec<Member> },
}

enum Member {
    Single(char),
    Range(char, char),
}

pub fn matches(pattern: &str, text: &str) -> bool {
    let tokens = tokenize(pattern);
    let text: Vec<char> = text.chars().collect();
    matches_tokens(&tokens, &text)
}

fn tokenize(pattern: &str) -> Vec<Token> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let (token, consumed) = match chars[index] {
            '*' => (Token::Star, 1),
            '?' => (Token::Any, 1),
            '[' => parse_set(&chars[index..]).unwrap_or((Token::Literal('['), 1)),
            other => (Token::Literal(other), 1),
        };
        tokens.push(token);
        index += consumed;
    }
    tokens
}

fn parse_set(chars: &[char]) -> Option<(Token, usize)> {
    let negated = chars.get(1) == Some(&'!');
    let start = if negated { 2 } else { 1 };
    let first_member = if chars.get(start) == Some(&']') {
        start + 1
    } else {
        start
    };
    let close = chars[first_member..].iter().position(|c| *c == ']')? + first_member;
    let members = parse_members(&chars[start..close]);
    Some((Token::Set { negated, members }, close + 1))
}

fn parse_members(chars: &[char]) -> Vec<Member> {
    let mut members = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars.get(index + 1) == Some(&'-') && index + 2 < chars.len() {
            members.push(Member::Range(chars[index], chars[index + 2]));
            index += 3;
        } else {
            members.push(Member::Single(chars[index]));
            index += 1;
        }
    }
    members
}

fn token_accepts(token: &Token, character: char) -> bool {
    match token {
        Token::Star => false,
        Token::Any => true,
        Token::Literal(expected) => *expected == character,
        Token::Set { negated, members } => {
            members.iter().any(|member| member.contains(character)) != *negated
        }
    }
}

impl Member {
    fn contains(&self, character: char) -> bool {
        match self {
            Member::Single(single) => *single == character,
            Member::Range(low, high) => (*low..=*high).contains(&character),
        }
    }
}

fn matches_tokens(tokens: &[Token], text: &[char]) -> bool {
    let (mut token_index, mut text_index) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while text_index < text.len() {
        match tokens.get(token_index) {
            Some(Token::Star) => {
                backtrack = Some((token_index, text_index));
                token_index += 1;
            }
            Some(token) if token_accepts(token, text[text_index]) => {
                token_index += 1;
                text_index += 1;
            }
            _ => match backtrack {
                Some((star_index, star_text)) => {
                    backtrack = Some((star_index, star_text + 1));
                    token_index = star_index + 1;
                    text_index = star_text + 1;
                }
                None => return false,
            },
        }
    }
    tokens[token_index..]
        .iter()
        .all(|token| matches!(token, Token::Star))
}
