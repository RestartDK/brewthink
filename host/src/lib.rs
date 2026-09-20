pub mod machine;
pub mod memory;
pub mod stack;

pub(crate) fn number(text: &str) -> anyhow::Result<i64> {
    let (sign, text) = text.strip_prefix('-').map_or((1, text), |s| (-1, s));
    Ok(sign
        * if let Some(hex) = text.strip_prefix("0x") {
            i64::from_str_radix(hex, 16)?
        } else {
            text.parse()?
        })
}
