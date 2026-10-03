//! A [Bowtie](https://github.com/bowtie-json-schema/bowtie) harness for corvus-json-schema. It speaks IHOP (one JSON
//! request per line on standard input, one response per line on standard output):
//!
//! - `start` reports the implementation and its dialects;
//! - `dialect` sets the dialect for schemas without `$schema`;
//! - `run` compiles the case's schema with the case's `registry` as the document resolver and validates each instance
//!   (for `annotations` output, through a verbose results collector, reporting each annotation with its instance
//!   location and `#…` keyword location);
//! - `stop` exits.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use corvus_json_schema::{
    CompileOptions, Dialect, JsonSchemaResultsCollector, ResultsLevel, collect_annotations as collector_annotations,
    compile_with,
};
use serde_json::{Map, Value, json};

const DIALECTS: [(&str, Dialect); 5] = [
    ("https://json-schema.org/draft/2020-12/schema", Dialect::Draft202012),
    ("https://json-schema.org/draft/2019-09/schema", Dialect::Draft201909),
    ("http://json-schema.org/draft-07/schema#", Dialect::Draft7),
    ("http://json-schema.org/draft-06/schema#", Dialect::Draft6),
    ("http://json-schema.org/draft-04/schema#", Dialect::Draft4),
];

fn errored(message: impl Into<String>) -> Value {
    json!({ "errored": true, "context": { "message": message.into() } })
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// The annotations a verbose collector grouped (instance location, then keyword as a JSON-pointer token, then schema
/// location as a `#…` fragment; the same in every Corvus implementation) as Bowtie lists them: each with its keyword
/// unescaped, and its keyword location the schema location's fragment followed by `/` and the keyword token,
/// percent-encoded as the fragment is.
fn annotations_of(grouped: &BTreeMap<String, BTreeMap<String, BTreeMap<String, Value>>>) -> Vec<Value> {
    let mut found = Vec::new();
    for (instance_location, keywords) in grouped {
        for (token, locations) in keywords {
            for (schema_location, value) in locations {
                found.push(json!({
                    "keyword": token.replace("~1", "/").replace("~0", "~"),
                    "instanceLocation": instance_location,
                    "keywordLocation": format!("{schema_location}/{}", percent_encode(token)),
                    "annotation": value,
                }));
            }
        }
    }
    found
}

/// Percent-encodes text as a URI fragment does (upper-case hex, UTF-8), keeping the characters a fragment allows.
fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for &byte in text.as_bytes() {
        let c = byte as char;
        if byte < 128 && (c.is_ascii_alphanumeric() || "-._~!$&'()*+,;=:@/?".contains(c)) {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn strip_fragment(uri: &str) -> &str {
    uri.split_once('#').map_or(uri, |(base, _)| base)
}

struct Harness {
    started: bool,
    dialect: Dialect,
}

impl Harness {
    fn start(&mut self, request: &Value) -> Result<Value, String> {
        if request["version"] != 1 {
            return Err(format!("Unsupported IHOP version {}", request["version"]));
        }
        self.started = true;
        let os_version = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        Ok(json!({
            "version": 1,
            "implementation": {
                "language": "rust",
                "name": "corvus-json-schema",
                "version": corvus_json_schema::VERSION,
                "homepage": "https://github.com/corvus-dotnet/Corvus.JsonSchema",
                "documentation": "https://docs.rs/corvus-json-schema",
                "issues": "https://github.com/corvus-dotnet/Corvus.JsonSchema/issues",
                "source": "https://github.com/corvus-dotnet/Corvus.JsonSchema",
                "dialects": DIALECTS.iter().map(|(uri, _)| *uri).collect::<Vec<_>>(),
                "os": std::env::consts::OS,
                "os_version": os_version.trim(),
                "language_version": env!("HARNESS_RUSTC_VERSION"),
            },
        }))
    }

    fn dialect(&mut self, request: &Value) -> Result<Value, String> {
        self.require_started()?;
        let uri = request["dialect"].as_str().unwrap_or_default();
        match DIALECTS.iter().find(|(u, _)| *u == uri) {
            Some((_, d)) => {
                self.dialect = *d;
                Ok(json!({ "ok": true }))
            }
            None => Ok(json!({ "ok": false })),
        }
    }

    fn run(&mut self, request: &Value) -> Result<Value, String> {
        self.require_started()?;
        let case = &request["case"];
        let registry: HashMap<String, Value> = case["registry"]
            .as_object()
            .map(|r| r.iter().map(|(uri, schema)| (strip_fragment(uri).to_string(), schema.clone())).collect())
            .unwrap_or_default();
        let registry = Arc::new(registry);
        let options = CompileOptions {
            default_dialect: self.dialect,
            resolve_document: Some(Arc::new(move |uri: &str| registry.get(strip_fragment(uri)).cloned())),
            ..CompileOptions::default()
        };
        let seq = request["seq"].clone();
        let validator = match catch_unwind(AssertUnwindSafe(|| compile_with(&case["schema"], &options))) {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Ok(with_seq(seq, errored(e.message()))),
            Err(p) => return Ok(with_seq(seq, errored(panic_message(&*p)))),
        };
        let annotations = request["output"] == "annotations";
        let tests = case["tests"].as_array().map(Vec::as_slice).unwrap_or_default();
        let results: Vec<Value> = tests
            .iter()
            .map(|test| {
                let instance = &test["instance"];
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    if !annotations {
                        return validator.validate(instance).map(|valid| json!({ "valid": valid }));
                    }
                    let mut collector = JsonSchemaResultsCollector::new(ResultsLevel::Verbose);
                    let valid = validator.evaluate(instance, &mut collector)?;
                    let found = annotations_of(&collector_annotations(&collector));
                    Ok(json!({ "valid": valid, "annotations": found }))
                }));
                match outcome {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => errored("evaluation recursed beyond the maximum depth"),
                    Err(p) => errored(panic_message(&*p)),
                }
            })
            .collect();
        Ok(json!({ "seq": seq, "results": results }))
    }

    fn require_started(&self) -> Result<(), String> {
        if self.started { Ok(()) } else { Err("Not started".into()) }
    }
}

fn with_seq(seq: Value, mut response: Value) -> Value {
    let mut out = Map::new();
    out.insert("seq".into(), seq);
    out.append(response.as_object_mut().expect("an object"));
    Value::Object(out)
}

fn main() {
    // Panics are reported to Bowtie as errors, not printed over the protocol's output.
    std::panic::set_hook(Box::new(|_| {}));
    let mut harness = Harness { started: false, dialect: Dialect::Draft202012 };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("read a request");
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line).expect("parse a request");
        let response = match request["cmd"].as_str() {
            Some("start") => harness.start(&request),
            Some("dialect") => harness.dialect(&request),
            Some("run") => harness.run(&request),
            Some("stop") => {
                if harness.started {
                    return;
                }
                Err("Not started".into())
            }
            other => Err(format!("Unknown command {other:?}")),
        };
        match response {
            Ok(response) => {
                writeln!(stdout, "{response}").expect("write a response");
                stdout.flush().expect("flush");
            }
            Err(message) => panic!("{message}"),
        }
    }
}
