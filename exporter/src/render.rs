use std::{collections::HashMap, io::Write as _};

use anyhow::Context as _;
use bytes::{BufMut as _, BytesMut};
use serde::Serialize;
use serde_prom::MetricDescriptor;

use cgroups_exporter_config::MetricsConfig;

const NAME_LABEL: &str = "name";

type Metadata<'s> = HashMap<&'s str, MetricDescriptor<'s>>;
type Serializer<'s> = serde_prom::PrometheusSerializer<'s>;

pub struct MetricsRenderer<'s> {
    common_labels: Vec<(&'s str, &'s str)>,
    metadata: &'s Metadata<'s>,
    /// Prometheus serializer for each namespace.
    serializers: HashMap<Option<String>, Serializer<'s>>,
}

impl<'s> MetricsRenderer<'s> {
    pub fn new(common_labels: Vec<(&'s str, &'s str)>, metadata: &'s Metadata<'s>) -> Self {
        Self {
            common_labels: common_labels.clone(),
            metadata,
            serializers: HashMap::from([(
                None,
                serde_prom::PrometheusSerializer::new(None::<String>, metadata, common_labels),
            )]),
        }
    }

    pub fn finish(self) -> anyhow::Result<Vec<u8>> {
        let mut writer = BytesMut::new().writer();
        for (_, serializer) in self.serializers {
            serializer
                .finish(&mut writer)
                .context("Failed to finish serialization")?;
            writer.write_all(b"\n")?;
        }
        Ok(writer.into_inner().into())
    }

    pub fn render(
        &mut self,
        match_group: MatchGroup<impl Serialize + Named>,
    ) -> anyhow::Result<()> {
        let MatchGroup {
            data,
            metrics_config,
        } = match_group;

        for metric in data {
            let serializer = self.serializer(&metrics_config.namespace);
            let metric_name = metric.name().to_string();
            let name_label = metrics_config
                .label_map
                .get(NAME_LABEL)
                .map_or_else(|| NAME_LABEL.to_string(), std::borrow::ToOwned::to_owned);
            serializer.set_current_labels(vec![(name_label, metric_name)]);
            metric.serialize(serializer)?;
        }

        Ok(())
    }

    #[allow(clippy::ref_option)]
    fn serializer(&mut self, namespace: &Option<String>) -> &mut Serializer<'s> {
        self.serializers
            .entry(namespace.to_owned())
            .or_insert_with(|| {
                serde_prom::PrometheusSerializer::new(
                    namespace.to_owned(),
                    self.metadata,
                    self.common_labels.clone(),
                )
            })
    }
}

pub struct MatchGroup<T> {
    data: Vec<T>,
    metrics_config: MetricsConfig,
}

impl<T> MatchGroup<T> {
    pub fn new(data: Vec<T>, metrics_config: MetricsConfig) -> Self {
        Self {
            data,
            metrics_config,
        }
    }

    pub fn insert(&mut self, data: T) {
        self.data.push(data);
    }

    pub fn into_parts(self) -> (Vec<T>, MetricsConfig) {
        (self.data, self.metrics_config)
    }
}

pub trait Named {
    fn name(&self) -> &str;
}
