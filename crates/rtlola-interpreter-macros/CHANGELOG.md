# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - T.B.A

### Added
- Derive macro ```FromStreamValues``` to implement the `FromValues` trait for structs with named fields that implement `TryFrom<Value>`.

## [0.1.0] - 28.06.2024

### Added
- Derive macro ```ValueFactory``` to derive the `AssociateFactory` trait for a type with fields that implement `TryInto<Value>` 
- Derive macro ```CompositFactory``` to derive the `AssociateFactory` trait for a type with fields that implement `AssociateFactory` 
