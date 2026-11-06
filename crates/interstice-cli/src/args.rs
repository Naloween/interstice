//! Command-line arguments typed by the function's signature: numbers,
//! booleans and strings as plain text; structs, enums, lists, options and
//! tuples as JSON (`{"x":1}`, `"Unit"` or `{"Variant": value}`, `[1,2]`,
//! `null`).

use interstice_core::interstice_abi::{
    FieldDef, IntersticeType, IntersticeTypeDef, IntersticeValue, ModuleSchema,
};
use serde_json::Value;
use std::collections::HashMap;

type Defs = HashMap<String, IntersticeTypeDef>;

/// `name(a: u32, b: String)`, for messages.
pub fn signature(name: &str, params: &[FieldDef]) -> String {
    let params: Vec<String> = params.iter().map(|p| format!("{}: {}", p.name, p.field_type)).collect();
    format!("{name}({})", params.join(", "))
}

/// Parse `raw` against `params`.
pub fn parse(name: &str, raw: &[String], params: &[FieldDef], defs: &Defs) -> Result<Vec<IntersticeValue>, String> {
    if raw.len() != params.len() {
        return Err(format!(
            "{} takes {} argument(s), got {}",
            signature(name, params),
            params.len(),
            raw.len()
        ));
    }
    raw.iter()
        .zip(params)
        .map(|(text, param)| {
            parse_one(text, &param.field_type, defs)
                .map_err(|e| format!("argument '{}' of {}: {e}", param.name, signature(name, params)))
        })
        .collect()
}

/// The parameters of `function` (a query if `query`) in `module`.
pub fn params<'a>(module: &'a ModuleSchema, function: &str, query: bool) -> Option<&'a [FieldDef]> {
    if query {
        module.queries.iter().find(|q| q.name == function).map(|q| q.arguments.as_slice())
    } else {
        module.reducers.iter().find(|r| r.name == function).map(|r| r.arguments.as_slice())
    }
}

fn parse_one(text: &str, ty: &IntersticeType, defs: &Defs) -> Result<IntersticeValue, String> {
    let num = |e: std::num::ParseIntError| format!("{text:?} is not a {ty}: {e}");
    Ok(match ty {
        IntersticeType::String => IntersticeValue::String(text.to_string()),
        IntersticeType::U8 => IntersticeValue::U8(text.parse().map_err(num)?),
        IntersticeType::U32 => IntersticeValue::U32(text.parse().map_err(num)?),
        IntersticeType::U64 => IntersticeValue::U64(text.parse().map_err(num)?),
        IntersticeType::I32 => IntersticeValue::I32(text.parse().map_err(num)?),
        IntersticeType::I64 => IntersticeValue::I64(text.parse().map_err(num)?),
        IntersticeType::F32 => IntersticeValue::F32(text.parse().map_err(|e| format!("{text:?}: {e}"))?),
        IntersticeType::F64 => IntersticeValue::F64(text.parse().map_err(|e| format!("{text:?}: {e}"))?),
        IntersticeType::Bool => IntersticeValue::Bool(text.parse().map_err(|_| format!("{text:?} is not true or false"))?),
        _ => {
            let json: Value = serde_json::from_str(text).map_err(|e| format!("expected JSON for {ty}: {e}"))?;
            from_json(&json, ty, defs)?
        }
    })
}

fn from_json(json: &Value, ty: &IntersticeType, defs: &Defs) -> Result<IntersticeValue, String> {
    let wrong = || format!("expected {ty}, got {json}");
    let int = |v: Option<i128>| v.ok_or_else(wrong);
    let as_int = || json.as_i64().map(i128::from).or(json.as_u64().map(i128::from));
    let fit = |v: i128, ok: bool| if ok { Ok(v) } else { Err(format!("{v} does not fit {ty}")) };
    Ok(match ty {
        IntersticeType::Void => IntersticeValue::Void,
        IntersticeType::U8 => { let v = int(as_int())?; IntersticeValue::U8(fit(v, (0..=255).contains(&v))? as u8) }
        IntersticeType::U32 => { let v = int(as_int())?; IntersticeValue::U32(fit(v, (0..=u32::MAX as i128).contains(&v))? as u32) }
        IntersticeType::U64 => { let v = int(as_int())?; IntersticeValue::U64(fit(v, (0..=u64::MAX as i128).contains(&v))? as u64) }
        IntersticeType::I32 => { let v = int(as_int())?; IntersticeValue::I32(fit(v, (i32::MIN as i128..=i32::MAX as i128).contains(&v))? as i32) }
        IntersticeType::I64 => { let v = int(as_int())?; IntersticeValue::I64(fit(v, (i64::MIN as i128..=i64::MAX as i128).contains(&v))? as i64) }
        IntersticeType::F32 => IntersticeValue::F32(json.as_f64().ok_or_else(wrong)? as f32),
        IntersticeType::F64 => IntersticeValue::F64(json.as_f64().ok_or_else(wrong)?),
        IntersticeType::Bool => IntersticeValue::Bool(json.as_bool().ok_or_else(wrong)?),
        IntersticeType::String => IntersticeValue::String(json.as_str().ok_or_else(wrong)?.to_string()),
        IntersticeType::Vec(inner) => IntersticeValue::Vec(
            json.as_array().ok_or_else(wrong)?.iter().map(|v| from_json(v, inner, defs)).collect::<Result<_, _>>()?,
        ),
        IntersticeType::Option(inner) => match json {
            Value::Null => IntersticeValue::Option(None),
            v => IntersticeValue::Option(Some(Box::new(from_json(v, inner, defs)?))),
        },
        IntersticeType::Tuple(items) => {
            let arr = json.as_array().filter(|a| a.len() == items.len()).ok_or_else(wrong)?;
            IntersticeValue::Tuple(arr.iter().zip(items).map(|(v, t)| from_json(v, t, defs)).collect::<Result<_, _>>()?)
        }
        IntersticeType::Named(name) => match defs.get(name) {
            Some(IntersticeTypeDef::Struct { name, fields }) => {
                let obj = json.as_object().ok_or_else(wrong)?;
                let mut out = Vec::with_capacity(fields.len());
                for f in fields {
                    let v = obj.get(&f.name).ok_or_else(|| format!("{name} needs field '{}'", f.name))?;
                    out.push(from_json(v, &f.field_type, defs)?);
                }
                IntersticeValue::Struct(out)
            }
            Some(IntersticeTypeDef::Enum { name, variants }) => {
                let (variant, payload) = match json {
                    Value::String(v) => (v.clone(), Value::Null),
                    Value::Object(o) if o.len() == 1 => {
                        let (k, v) = o.iter().next().unwrap();
                        (k.clone(), v.clone())
                    }
                    _ => return Err(format!("expected a variant of {name}, as \"Variant\" or {{\"Variant\": value}}")),
                };
                let index = variants
                    .iter()
                    .position(|v| v.name == variant)
                    .ok_or_else(|| format!("{name} has no variant {variant}"))?;
                IntersticeValue::Enum {
                    variant: index as u32,
                    value: Box::new(from_json(&payload, &variants[index].field_type, defs)?),
                }
            }
            None => return Err(format!("unknown type {name}")),
        },
        IntersticeType::Blob => return Err("blob arguments cannot be given on the command line".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, t: IntersticeType) -> FieldDef {
        FieldDef { name: name.into(), field_type: t }
    }

    #[test]
    fn typed_arguments() {
        let defs = Defs::new();
        let params = [p("count", IntersticeType::U32), p("balance", IntersticeType::I64), p("tags", IntersticeType::Vec(Box::new(IntersticeType::String)))];
        let raw = ["10".to_string(), "-5".to_string(), r#"["a","b"]"#.to_string()];
        let v = parse("seed", &raw, &params, &defs).unwrap();
        assert_eq!(v[0], IntersticeValue::U32(10));
        assert_eq!(v[1], IntersticeValue::I64(-5));
        assert_eq!(v[2], IntersticeValue::Vec(vec!["a".to_string().into(), "b".to_string().into()]));
        let err = parse("seed", &raw[..1], &params, &defs).unwrap_err();
        assert!(err.contains("seed(count: u32, balance: i64, tags: Vec<String>)"), "{err}");
        assert!(parse("seed", &["x".into(), "1".into(), "[]".into()], &params, &defs).unwrap_err().contains("count"));
    }
}
