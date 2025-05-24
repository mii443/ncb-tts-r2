use opentelemetry::{
    global,
    trace::{SamplingDecision, SamplingResult, TraceContextExt, TraceState, TracerProvider as _},
    KeyValue,
};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::{
    metrics::{MeterProviderBuilder, PeriodicReader, SdkMeterProvider},
    trace::{RandomIdGenerator, SdkTracerProvider, ShouldSample},
    Resource,
};
use tracing::Level;
use tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone)]
struct FilterSampler;

impl ShouldSample for FilterSampler {
    fn should_sample(
        &self,
        parent_context: Option<&opentelemetry::Context>,
        _trace_id: opentelemetry::TraceId,
        name: &str,
        _span_kind: &opentelemetry::trace::SpanKind,
        _attributes: &[KeyValue],
        _links: &[opentelemetry::trace::Link],
    ) -> opentelemetry::trace::SamplingResult {
        let decision = if name == "dispatch" || name == "recv_event" {
            SamplingDecision::Drop
        } else {
            SamplingDecision::RecordAndSample
        };

        SamplingResult {
            decision,
            attributes: vec![],
            trace_state: match parent_context {
                Some(ctx) => ctx.span().span_context().trace_state().clone(),
                None => TraceState::default(),
            },
        }
    }
}

fn resource() -> Resource {
    Resource::builder().with_service_name("ncb-tts-r2").build()
}

fn init_meter_provider(url: &str) -> SdkMeterProvider {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .with_endpoint(url)
        .with_protocol(Protocol::HttpBinary)
        .with_temporality(opentelemetry_sdk::metrics::Temporality::default())
        .build()
        .unwrap();

    let reader = PeriodicReader::builder(exporter)
        .with_interval(std::time::Duration::from_secs(5))
        .build();

    let stdout_reader =
        PeriodicReader::builder(opentelemetry_stdout::MetricExporter::default()).build();

    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource())
        .with_reader(reader)
        .with_reader(stdout_reader)
        .build();

    global::set_meter_provider(meter_provider.clone());

    meter_provider
}

fn init_tracer_provider(url: &str) -> SdkTracerProvider {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(url)
        .with_protocol(Protocol::HttpBinary)
        .build()
        .unwrap();

    SdkTracerProvider::builder()
        .with_sampler(FilterSampler)
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource())
        .with_batch_exporter(exporter)
        .build()
}

pub fn init_tracing_subscriber(otel_http_url: &Option<String>) -> OtelGuard {
    let registry = tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            Level::INFO,
        ))
        .with(tracing_subscriber::fmt::layer());

    if let Some(url) = otel_http_url {
        let tracer_provider = init_tracer_provider(url);
        let meter_provider = init_meter_provider(url);

        let tracer = tracer_provider.tracer("ncb-tts-r2");

        registry
            .with(MetricsLayer::new(meter_provider.clone()))
            .with(OpenTelemetryLayer::new(tracer))
            .init();

        OtelGuard {
            _tracer_provider: Some(tracer_provider),
            _meter_provider: Some(meter_provider),
        }
    } else {
        registry.init();

        OtelGuard {
            _tracer_provider: None,
            _meter_provider: None,
        }
    }
}

pub struct OtelGuard {
    _tracer_provider: Option<SdkTracerProvider>,
    _meter_provider: Option<SdkMeterProvider>,
}
