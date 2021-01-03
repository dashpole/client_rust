//! Module implementing an Open Metrics metric family.
//!
//! See [`Family`] for details.

use super::{MetricType, TypedMetric};
use owning_ref::OwningRef;
use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard};

/// Representation of the OpenMetrics *MetricFamily* data type.
///
/// A [`Family`] is a set of metrics with the same name, help text and
/// type, differentiated by their label values thus spanning a multidimensional
/// space.
///
/// # Generic over the label set
///
/// A [`Family`] is generic over the label type. For convenience one might
/// choose a `Vec<(String, String)>`, for performance one might define a custom
/// type.
///
/// ## Examples
///
/// ### [`Family`] with `Vec<(String, String)>` for convenience
///
/// ```
/// # use open_metrics_client::encoding::text::encode;
/// # use open_metrics_client::metrics::counter::{Atomic, Counter};
/// # use open_metrics_client::metrics::family::Family;
/// # use open_metrics_client::registry::{Descriptor, Registry};
/// # use std::sync::atomic::AtomicU64;
/// #
/// # let mut registry = Registry::default();
/// let family = Family::<Vec<(String, String)>, Counter<AtomicU64>>::default();
/// # registry.register(
/// #   "my_counter",
/// #   "This is my counter",
/// #   family.clone(),
/// # );
///
/// // Record a single HTTP GET request.
/// family.get_or_create(&vec![("method".to_owned(), "GET".to_owned())]).inc();
///
/// # // Encode all metrics in the registry in the text format.
/// # let mut buffer = vec![];
/// # encode(&mut buffer, &registry).unwrap();
/// #
/// # let expected = "# HELP my_counter This is my counter.\n".to_owned() +
/// #                "# TYPE my_counter counter\n" +
/// #                "my_counter_total{method=\"GET\"} 1\n" +
/// #                "# EOF\n";
/// # assert_eq!(expected, String::from_utf8(buffer).unwrap());
/// ```
///
/// ### [`Family`] with custom type for performance
///
/// ```
/// # use open_metrics_client::encoding::text::Encode;
/// # use open_metrics_client::encoding::text::encode;
/// # use open_metrics_client::metrics::counter::{Atomic, Counter};
/// # use open_metrics_client::metrics::family::Family;
/// # use open_metrics_client::registry::{Descriptor, Registry};
/// # use std::io::Write;
/// # use std::sync::atomic::AtomicU64;
/// #
/// # let mut registry = Registry::default();
/// #[derive(Clone, Hash, PartialEq, Eq)]
/// struct Labels {
///   method: Method,
/// };
///
/// #[derive(Clone, Hash, PartialEq, Eq)]
/// enum Method {
///   Get,
///   Put,
/// };
///
/// # impl Encode for Labels {
/// #   fn encode(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
/// #     let method = match self.method {
/// #         Method::Get => {
/// #             b"method=\"GET\""
/// #         }
/// #         Method::Put => {
/// #             b"method=\"PUT\""
/// #         }
/// #     };
/// #     writer.write_all(method).map(|_| ())
/// #   }
/// # }
/// #
/// let family = Family::<Labels, Counter<AtomicU64>>::default();
/// # registry.register(
/// #   "my_counter",
/// #   "This is my counter",
/// #   family.clone(),
/// # );
///
/// // Record a single HTTP GET request.
/// family.get_or_create(&Labels { method: Method::Get }).inc();
/// #
/// # // Encode all metrics in the registry in the text format.
/// # let mut buffer = vec![];
/// # encode(&mut buffer, &registry).unwrap();
///
/// # let expected = "# HELP my_counter This is my counter.\n".to_owned() +
/// #                "# TYPE my_counter counter\n" +
/// #                "my_counter_total{method=\"GET\"} 1\n" +
/// #                "# EOF\n";
/// # assert_eq!(expected, String::from_utf8(buffer).unwrap());
/// ```
pub struct Family<S, M> {
    metrics: Arc<RwLock<HashMap<S, M>>>,
    /// Function that when called constructs a new metric.
    ///
    /// For most metric types this would simply be its [`Default`]
    /// implementation set through [`Family::default`]. For metric types that
    /// need custom construction logic like
    /// [`Histogram`](crate::metrics::histogram::Histogram) in order to set
    /// specific buckets, a custom constructor is set via
    /// [`Family::new_with_constructor`].
    constructor: fn() -> M,
}

impl<S: Clone + std::hash::Hash + Eq, M: Default> Default for Family<S, M> {
    fn default() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(Default::default())),
            constructor: M::default,
        }
    }
}

impl<S: Clone + std::hash::Hash + Eq, M> Family<S, M> {
    pub fn new_with_constructor(constructor: fn() -> M) -> Self {
        Self {
            metrics: Arc::new(RwLock::new(Default::default())),
            constructor,
        }
    }
}

impl<S: Clone + std::hash::Hash + Eq, M> Family<S, M> {
    pub fn get_or_create(&self, sample_set: &S) -> OwningRef<RwLockReadGuard<HashMap<S, M>>, M> {
        let read_guard = self.metrics.read().unwrap();
        if let Ok(metric) =
            OwningRef::new(read_guard).try_map(|metrics| metrics.get(sample_set).ok_or(()))
        {
            return metric;
        }

        let mut write_guard = self.metrics.write().unwrap();
        write_guard.insert(sample_set.clone(), (self.constructor)());

        drop(write_guard);

        let read_guard = self.metrics.read().unwrap();
        OwningRef::new(read_guard).map(|metrics| {
            metrics
                .get(sample_set)
                .expect("Metric to exist after creating it.")
        })
    }

    pub(crate) fn read(&self) -> RwLockReadGuard<HashMap<S, M>> {
        self.metrics.read().unwrap()
    }
}

impl<S, M> Clone for Family<S, M> {
    fn clone(&self) -> Self {
        Family {
            metrics: self.metrics.clone(),
            constructor: self.constructor,
        }
    }
}

impl<S, M: TypedMetric> TypedMetric for Family<S, M> {
    const TYPE: MetricType = <M as TypedMetric>::TYPE;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::counter::Counter;
    use crate::metrics::histogram::{exponential_series, Histogram};
    use std::sync::atomic::AtomicU64;

    #[test]
    fn counter_family() {
        let family = Family::<Vec<(String, String)>, Counter<AtomicU64>>::default();

        family
            .get_or_create(&vec![("method".to_string(), "GET".to_string())])
            .inc();

        assert_eq!(
            1,
            family
                .get_or_create(&vec![("method".to_string(), "GET".to_string())])
                .get()
        );
    }

    #[test]
    fn histogram_family() {
        Family::<(), Histogram>::new_with_constructor(|| {
            Histogram::new(exponential_series(1.0, 2.0, 10))
        });
    }
}