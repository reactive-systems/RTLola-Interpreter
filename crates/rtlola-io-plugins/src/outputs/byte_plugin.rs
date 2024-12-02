//! Module that contains the implementation of the [VerdictsSink] for bytes.
use std::error::Error;
use std::marker::PhantomData;

use rtlola_interpreter::monitor::VerdictRepresentation;
use rtlola_interpreter::output::VerdictFactory;
use rtlola_interpreter::time::OutputTimeRepresentation;
use serde::Serialize;

use super::VerdictsSink;

/// Parses and send bytes with a [VerdictFactory], [ByteSerializer] and a [ByteTarget].
#[derive(Debug)]
pub struct BincodeSink<
    V: VerdictRepresentation,
    T: OutputTimeRepresentation,
    Factory: VerdictFactory<V, T>,
    Serializer: ByteSerializer,
    Target: ByteTarget,
> {
    factory: Factory,
    serializer: Serializer,
    target: Target,
    phantom: PhantomData<(V, T)>,
}

impl<
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T>,
        Serializer: ByteSerializer<Input = Factory::Verdict>,
        Target: ByteTarget,
    > BincodeSink<V, T, Factory, Serializer, Target>
{
    /// Creates a new [ByteVerdictSink] out of a (target)[ByteTarget], a (factory)[VerdictFactory] and a (serializer)[ByteSerializer].
    pub fn new(target: Target, factory: Factory, serializer: Serializer) -> Self {
        Self {
            serializer,
            factory,
            target,
            phantom: PhantomData,
        }
    }
}

impl<
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T>,
        Serializer: ByteSerializer<Input = Factory::Verdict>,
        Target: ByteTarget,
    > VerdictsSink<V, T> for BincodeSink<V, T, Factory, Serializer, Target>
{
    type Error = ByteVerdictSinkError<Factory::Error, Serializer::Error, Target::Error>;
    type Factory = Factory;
    type Return = ();

    fn sink(
        &mut self,
        verdict: <Self::Factory as VerdictFactory<V, T>>::Verdict,
    ) -> Result<Self::Return, Self::Error> {
        let buffer = self
            .serializer
            .into_bytes(verdict)
            .map_err(ByteVerdictSinkError::Serializer)?;
        self.target
            .write(&buffer)
            .map_err(ByteVerdictSinkError::Target)
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

/// Enum to collect the errors with for a [ByteVerdictSink].
#[derive(Debug)]
pub enum ByteVerdictSinkError<
    Factory: Error + 'static,
    Serializer: Error + 'static,
    Target: Error + 'static,
> {
    #[allow(missing_docs)]
    Factory(Factory),
    #[allow(missing_docs)]
    Target(Target),
    #[allow(missing_docs)]
    Serializer(Serializer),
}

impl<Factory: Error + 'static, Serializer: Error + 'static, Target: Error + 'static>
    std::fmt::Display for ByteVerdictSinkError<Factory, Serializer, Target>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ByteVerdictSinkError::Factory(e) => writeln!(f, "{e}"),
            ByteVerdictSinkError::Target(e) => writeln!(f, "{e}"),
            ByteVerdictSinkError::Serializer(e) => writeln!(f, "{e}"),
        }
    }
}

impl<Factory: Error + 'static, Serializer: Error + 'static, Target: Error + 'static> Error
    for ByteVerdictSinkError<Factory, Serializer, Target>
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ByteVerdictSinkError::Factory(e) => Some(e),
            ByteVerdictSinkError::Target(e) => Some(e),
            ByteVerdictSinkError::Serializer(e) => Some(e),
        }
    }
}

/// Trait to collect all connections for the [ByteEventSource]
pub trait ByteTarget {
    /// Error when receiving the bytestream
    type Error: Error + 'static;
    /// Function to receive the bytestream and returns the number of parsed bytes
    fn write(&mut self, buffer: &[u8]) -> Result<(), Self::Error>;
}

impl<T: std::io::Write> ByteTarget for T {
    type Error = std::io::Error;

    fn write(&mut self, buffer: &[u8]) -> Result<(), Self::Error> {
        std::io::Write::write_all(self, buffer)
    }
}

/// This trait defines a factory to create a bytestream from a verdict
pub trait ByteSerializer {
    /// Error when creating the bytestream
    type Error: Error + 'static;
    /// Type given to the serializer
    type Input;
    /// Function to create the bytestream
    #[allow(clippy::wrong_self_convention)]
    fn into_bytes(&mut self, verdict: Self::Input) -> Result<Vec<u8>, Self::Error>;
}

/// A struct to create a byte serializer that is build out of the [Serialize] trait.
#[derive(Debug, Clone, Copy, Default)]
pub struct SerdeByteSerializer<I: Serialize> {
    phantom: PhantomData<I>,
}

impl<I: Serialize> ByteSerializer for SerdeByteSerializer<I> {
    type Error = bincode::Error;
    type Input = I;

    fn into_bytes(&mut self, verdict: Self::Input) -> Result<Vec<u8>, Self::Error> {
        bincode::serialize(&verdict)
    }
}

impl<
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T> + Default,
        Serializer: ByteSerializer<Input = Factory::Verdict> + Default,
        Target: ByteTarget,
    > BincodeSink<V, T, Factory, Serializer, Target>
{
    /// Creates a new [ByteVerdictSink] given a [ByteTarget].
    pub fn from_target(target: Target) -> Self {
        Self::new(target, Factory::default(), Serializer::default())
    }
}

impl<
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T> + Default,
        Serializer: ByteSerializer<Input = Factory::Verdict> + Default,
        Target: ByteTarget,
    > From<Target> for BincodeSink<V, T, Factory, Serializer, Target>
{
    fn from(value: Target) -> Self {
        Self::from_target(value)
    }
}
