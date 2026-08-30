# Product

## Register

product

## Users

Brewthink's simulator is for firmware developers and contributors working on the Xteink X4. They use it during development to inspect exact logical frames, test image conversion, and exercise application behavior without repeatedly flashing physical hardware.

## Product Purpose

The simulator shortens the feedback loop between shared Rust rendering code and the physical X4. It should make differences between browser output and e-paper output obvious, produce X4-ready packed frames, and grow into a file-transfer tool without changing the rendering model.

## Brand Personality

Precise, tactile, restrained.

## Anti-references

Do not make this look like a generic SaaS dashboard, an analytics console, or a decorative imitation of paper. Avoid card grids, ornamental gradients, glass effects, novelty controls, and motion without state meaning.

## Design Principles

1. Keep the physical 480 × 800 display preview central.
2. Show exact rendering facts instead of abstract status summaries.
3. Use familiar developer-tool controls and direct manipulation.
4. Preserve parity with the X4 framebuffer over browser-specific convenience.
5. Add transfer behavior without turning the simulator into a separate product model.

## Accessibility & Inclusion

Target WCAG 2.2 AA. Support complete keyboard operation, visible focus, sufficient contrast, clear errors, semantic form labels, and reduced-motion preferences. Never rely on color alone to communicate state.
