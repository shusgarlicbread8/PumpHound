use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct RawEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub weight: f64,
    pub inferred: bool,
    pub event_id: Option<String>,
    pub signature: Option<String>,
}

pub fn aggregate_edges(raw: &[RawEdge]) -> Vec<Value> {
    let mut groups: BTreeMap<(String, String), PairAgg> = BTreeMap::new();
    for edge in raw {
        let key = (edge.source.clone(), edge.target.clone());
        let entry = groups.entry(key).or_insert_with(PairAgg::default);
        let weight = if edge.weight.is_finite() {
            edge.weight.max(0.0)
        } else {
            1.0
        };
        entry.weight += weight;
        entry.count += 1;
        entry.inferred |= edge.inferred;
        let mix = entry.mix.entry(edge.kind.clone()).or_insert((0.0, 0usize));
        mix.0 += weight;
        mix.1 += 1;
        entry.evidence.push(json!({
            "event_id": edge.event_id,
            "signature": edge.signature,
            "kind": edge.kind,
            "weight": edge.weight,
            "inferred": edge.inferred
        }));
    }
    groups
        .into_iter()
        .map(|((source, target), agg)| {
            let mix: Vec<Value> = agg
                .mix
                .into_iter()
                .map(|(kind, (weight, count))| json!({ "kind": kind, "weight": weight, "count": count }))
                .collect();
            let kind = mix
                .iter()
                .filter_map(|item| {
                    Some(format!(
                        "{}:{}",
                        item.get("kind")?.as_str()?,
                        item.get("count")?.as_u64()?
                    ))
                })
                .collect::<Vec<_>>()
                .join("+");
            json!({
                "source": source,
                "target": target,
                "kind": kind,
                "weight": agg.weight,
                "count": agg.count,
                "inferred": agg.inferred,
                "relationship_mix": mix,
                "evidence": agg.evidence
            })
        })
        .collect()
}

#[derive(Default)]
struct PairAgg {
    weight: f64,
    count: usize,
    inferred: bool,
    mix: BTreeMap<String, (f64, usize)>,
    evidence: Vec<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_relationships_collapse_without_dropping_evidence() {
        let raw = vec![
            RawEdge {
                source: "wallet:a".into(),
                target: "token:x".into(),
                kind: "BOUGHT".into(),
                weight: 2.0,
                inferred: false,
                event_id: Some("e1".into()),
                signature: Some("s1".into()),
            },
            RawEdge {
                source: "wallet:a".into(),
                target: "token:x".into(),
                kind: "BOUGHT".into(),
                weight: 3.0,
                inferred: false,
                event_id: Some("e2".into()),
                signature: Some("s2".into()),
            },
        ];
        let aggregated = aggregate_edges(&raw);
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0]["count"], 2);
        assert_eq!(aggregated[0]["weight"], 5.0);
        assert_eq!(aggregated[0]["evidence"].as_array().unwrap().len(), 2);
        assert_eq!(aggregated[0]["evidence"][0]["event_id"], "e1");
        assert_eq!(aggregated[0]["evidence"][1]["event_id"], "e2");
    }

    #[test]
    fn bought_and_sold_collapse_to_one_rendered_pair() {
        let raw = vec![
            RawEdge {
                source: "wallet:a".into(),
                target: "token:x".into(),
                kind: "BOUGHT".into(),
                weight: 2.0,
                inferred: false,
                event_id: Some("e1".into()),
                signature: Some("s1".into()),
            },
            RawEdge {
                source: "wallet:a".into(),
                target: "token:x".into(),
                kind: "SOLD".into(),
                weight: 1.0,
                inferred: false,
                event_id: Some("e2".into()),
                signature: Some("s2".into()),
            },
        ];
        let aggregated = aggregate_edges(&raw);
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0]["count"], 2);
        assert_eq!(aggregated[0]["evidence"].as_array().unwrap().len(), 2);
        let mix = aggregated[0]["relationship_mix"].as_array().unwrap();
        assert_eq!(mix.len(), 2);
    }
}
