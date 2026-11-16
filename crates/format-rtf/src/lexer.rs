//! Byte-oriented RTF tokenizer. Binary payloads are consumed before interpreting
//! braces; neither a picture nor an ignored destination can escape its group.
#[derive(Debug, PartialEq)]
pub enum Token<'a> { Open, Close, Word(&'a str,Option<i32>), Symbol(u8), Hex(u8), Text(&'a [u8]), Binary(&'a [u8]) }
pub struct Lexer<'a> { bytes:&'a[u8], at:usize }
impl<'a> Lexer<'a> {
    pub fn new(bytes:&'a[u8])->Self {Self{bytes,at:0}}
    pub fn next(&mut self)->Result<Option<Token<'a>>,String>{
        let Some(&b)=self.bytes.get(self.at)else{return Ok(None)};self.at+=1;
        Ok(Some(match b {
            b'{'=>Token::Open,b'}'=>Token::Close,
            b'\\'=>{let &c=self.bytes.get(self.at).ok_or("Truncated RTF escape")?;self.at+=1;
                if c==b'\'' {let s=self.bytes.get(self.at..self.at+2).ok_or("Truncated hex escape")?;self.at+=2;Token::Hex(hex(s[0])?.checked_mul(16).unwrap()+hex(s[1])?)}
                else if c.is_ascii_alphabetic(){let start=self.at-1;while self.bytes.get(self.at).is_some_and(u8::is_ascii_alphabetic){self.at+=1;if self.at-start>64{return Err("RTF control word is too long".into());}}
                    let word=std::str::from_utf8(&self.bytes[start..self.at]).map_err(|e|e.to_string())?;
                    let number_start=self.at; if self.bytes.get(self.at)==Some(&b'-'){self.at+=1;}
                    let digits=self.at;while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){self.at+=1;if self.at-digits>10{return Err("RTF parameter overflow".into());}}
                    let n=if digits<self.at{Some(std::str::from_utf8(&self.bytes[number_start..self.at]).map_err(|e|e.to_string())?.parse::<i32>().map_err(|_|"RTF parameter overflow")?)}else{if digits!=number_start{return Err("Missing signed RTF parameter".into());}None};
                    if self.bytes.get(self.at)==Some(&b' '){self.at+=1;}
                    if word=="bin"{let len=usize::try_from(n.ok_or("Missing binary length")?).map_err(|_|"Negative binary length")?;let end=self.at.checked_add(len).ok_or("Binary length overflow")?;let data=self.bytes.get(self.at..end).ok_or("Truncated RTF binary data")?;self.at=end;Token::Binary(data)}else{Token::Word(word,n)}
                }else{Token::Symbol(c)}
            },
            _=>{let start=self.at-1;while self.bytes.get(self.at).is_some_and(|b|!matches!(*b,b'{'|b'}'|b'\\')){self.at+=1;}Token::Text(&self.bytes[start..self.at])}
        }))
    }
}
pub fn hex(b:u8)->Result<u8,String>{match b{b'0'..=b'9'=>Ok(b-b'0'),b'a'..=b'f'=>Ok(b-b'a'+10),b'A'..=b'F'=>Ok(b-b'A'+10),_=>Err("Invalid hexadecimal RTF byte".into())}}
