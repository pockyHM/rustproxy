# Product

## Register

product

## Users

RustProxy is used by backend and operations engineers who manage routing behavior for services. They understand upstreams, HTTP headers, cookies, JWT claims, weights, priorities, and fallback behavior, and they need a configuration surface that helps them change traffic rules without losing sight of what will happen in production.

Their core job is to inspect, create, and edit reverse-proxy routing rules safely: define match conditions, map matched requests to upstream pools, tune weights, review YAML, and confirm service health from the same admin surface.

## Product Purpose

RustProxy is a high-performance Rust traffic routing middleware with a web admin UI for practical configuration. It exists to make rule-based routing understandable and editable while preserving an escape hatch through YAML when the UI is unavailable.

Success means an engineer can confidently answer three questions before saving a change: which requests match, where they will go, and what fallback behavior remains if nothing matches.

## Brand Personality

Reliable, clear, high-performance.

The interface should feel like infrastructure tooling built by people who operate real systems. It should be calm, direct, precise, and resistant to accidental misconfiguration. It should not feel decorative or theatrical; confidence comes from legible structure, explicit state, and careful interaction design.

## Anti-references

Do not make RustProxy look like a flashy SaaS landing page: no decorative gradients, vanity metric cards, or motion that competes with the task.

Do not copy large cloud-console patterns that bury actions in noisy navigation, overloaded tables, and inconsistent form hierarchies.

Do not use a terminal-native or hacker aesthetic as the primary visual language. Monospace belongs in YAML and metric previews, not as a personality substitute.

Do not drift toward low-code builder conventions. Rules should be structured and readable, not hidden behind drag-and-drop abstractions or visual programming metaphors.

## Design Principles

1. Prevent wrong saves before optimizing for speed. Rule priority, match conditions, upstream selection, and fallback behavior must be easy to review before changes are committed.
2. Show routing consequences, not just fields. Wherever possible, surface how a rule will match and where traffic will be sent.
3. Keep expert workflows direct. Engineers should not be slowed by onboarding copy once the model is clear.
4. Preserve the YAML escape hatch. The UI should complement manual configuration, not obscure or replace the underlying config model.
5. Make operational state plain. Health, metrics, empty states, and errors should be explicit, terse, and actionable.

## Accessibility & Inclusion

Target WCAG AA for the admin UI. Maintain readable contrast, keyboard-operable controls, visible focus states, clear form labels, and error messages that do not rely on color alone. Prefer reduced, purposeful motion and ensure dense routing data remains understandable when zoomed or read with assistive technology.
