use super::types::{BenchmarkRunConfig, TemplateContext};
use interstice_core::{
    IntersticeError,
    interstice_abi::{Field, IntersticeValue},
};
use serde_json::Value;

pub(crate) fn reducer_input_from_config(
    config: &BenchmarkRunConfig,
    context: &TemplateContext,
) -> Result<IntersticeValue, IntersticeError> {
    if let Some(input) = config.args_interstice_json.clone() {
        Ok(force_vec_args(input))
    } else {
        let templated = apply_template_to_json(&config.args_json, context);
        let value = json_to_interstice(templated)?;
        Ok(force_vec_args(value))
    }
}

pub(crate) fn query_input_from_config(
    config: &BenchmarkRunConfig,
    context: &TemplateContext,
) -> Result<IntersticeValue, IntersticeError> {
    if let Some(input) = config.verify.args_interstice_json.clone() {
        Ok(force_vec_args(input))
    } else {
        let templated = apply_template_to_json(&config.verify.args_json, context);
        let value = json_to_interstice(templated)?;
        Ok(force_vec_args(value))
    }
}

pub(crate) fn throughput_query_input_from_config(
    config: &BenchmarkRunConfig,
    context: &TemplateContext,
) -> Result<IntersticeValue, IntersticeError> {
    if let Some(input) = config.throughput_query_args_interstice_json.clone() {
        Ok(force_vec_args(input))
    } else {
        let templated = apply_template_to_json(&config.throughput_query_args_json, context);
        let value = json_to_interstice(templated)?;
        Ok(force_vec_args(value))
    }
}

fn force_vec_args(value: IntersticeValue) -> IntersticeValue {
    match value {
        IntersticeValue::Vec(_) => value,
        other => IntersticeValue::Vec(vec![other]),
    }
}

fn apply_template_to_json(value: &Value, context: &TemplateContext) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(flag) => Value::Bool(*flag),
        Value::Number(number) => Value::Number(number.clone()),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|entry| apply_template_to_json(entry, context))
                .collect(),
        ),
        Value::Object(object) => {
            let mut mapped = serde_json::Map::new();
            for (key, entry) in object {
                mapped.insert(key.clone(), apply_template_to_json(entry, context));
            }
            Value::Object(mapped)
        }
        Value::String(text) => template_string(text, context),
    }
}

fn template_string(text: &str, context: &TemplateContext) -> Value {
    match text {
        "$seq" => Value::Number(serde_json::Number::from(context.seq)),
        "$worker" => Value::Number(serde_json::Number::from(context.worker as u64)),
        "$op" => Value::Number(serde_json::Number::from(context.op)),
        "$now_ms" => Value::Number(serde_json::Number::from(context.now_ms)),
        "$max_seq" => Value::Number(serde_json::Number::from(context.max_seq)),
        "$total_sent" => Value::Number(serde_json::Number::from(context.total_sent)),
        "$client" => Value::String(context.client.clone()),
        "$max_client" => Value::String(context.max_client.clone()),
        _ => {
            let replaced = text
                .replace("$seq", &context.seq.to_string())
                .replace("$worker", &context.worker.to_string())
                .replace("$op", &context.op.to_string())
                .replace("$now_ms", &context.now_ms.to_string())
                .replace("$max_seq", &context.max_seq.to_string())
                .replace("$total_sent", &context.total_sent.to_string())
                .replace("$client", &context.client)
                .replace("$max_client", &context.max_client);
            Value::String(replaced)
        }
    }
}

fn json_to_interstice(value: Value) -> Result<IntersticeValue, IntersticeError> {
    match value {
        Value::Null => Ok(IntersticeValue::Void),
        Value::Bool(flag) => Ok(IntersticeValue::Bool(flag)),
        Value::Number(number) => {
            if let Some(integer) = number.as_u64() {
                Ok(IntersticeValue::U64(integer))
            } else if let Some(integer) = number.as_i64() {
                Ok(IntersticeValue::I64(integer))
            } else if let Some(float) = number.as_f64() {
                Ok(IntersticeValue::F64(float))
            } else {
                Err(IntersticeError::Internal("Unsupported JSON number".into()))
            }
        }
        Value::String(text) => Ok(IntersticeValue::String(text)),
        Value::Array(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                out.push(json_to_interstice(value)?);
            }
            Ok(IntersticeValue::Vec(out))
        }
        Value::Object(object) => {
            let mut fields = Vec::with_capacity(object.len());
            for (name, value) in object {
                fields.push(Field {
                    name,
                    value: json_to_interstice(value)?,
                });
            }
            Ok(IntersticeValue::Struct {
                name: "JsonObject".to_string(),
                fields,
            })
        }
    }
}

pub(crate) fn parse_json_value(json: &str) -> Result<Value, IntersticeError> {
    serde_json::from_str(json).map_err(|err| {
        IntersticeError::Internal(format!("Failed to parse JSON value '{}': {}", json, err))
    })
}

pub(crate) fn parse_interstice_json(json: &str) -> Result<IntersticeValue, IntersticeError> {
    serde_json::from_str(json).map_err(|err| {
        IntersticeError::Internal(format!(
            "Failed to parse IntersticeValue JSON '{}': {}",
            json, err
        ))
    })
}
