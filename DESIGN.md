---
name: RustProxy Admin
description: High-performance Rust traffic routing middleware admin interface
colors:
  neutral-bg: "#ffffff"
  neutral-surface: "#f6f8fa"
  neutral-border: "#ddd"
  neutral-border-subtle: "#eee"
  neutral-text: "#24292f"
  neutral-text-muted: "#57606a"
  muted-fill: "#f6f8fa"
typography:
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif"
    fontSize: "1rem"
    lineHeight: 1.5
  display:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif"
    fontSize: "1.75rem"
    fontWeight: 600
    lineHeight: 1.25
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
  mono:
    fontFamily: "'ui-monospace', 'SFMono-Regular', 'Menlo', 'Consolas', monospace"
    fontSize: "0.875rem"
    lineHeight: 1.5
rounded:
  sm: "4px"
  md: "8px"
spacing:
  sm: "0.25rem"
  md: "0.75rem"
  lg: "1rem"
  xl: "1.5rem"
components:
  button-primary:
    backgroundColor: "{colors.neutral-text}"
    textColor: "{colors.neutral-bg}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 1rem"
  button-primary-hover:
    backgroundColor: "#57606a"
    textColor: "{colors.neutral-bg}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.neutral-text}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 1rem"
  input-text:
    border: "1px solid {colors.neutral-border}"
    borderRadius: "{rounded.sm}"
    padding: "0.5rem"
  card-surface:
    backgroundColor: "{colors.neutral-bg}"
    border: "1px solid {colors.neutral-border}"
    borderRadius: "{rounded.md}"
    padding: "1rem"
  table-header:
    borderBottom: "1px solid {colors.neutral-border}"
    fontWeight: 600
    fontSize: "0.875rem"
---

# Design System: RustProxy Admin

## 1. Overview

**Creative North Star: "Control Room Ledger"**

RustProxy's admin interface is the operational ledger for traffic routing. It should feel like the console you'd find in a well-designed NOC or control room: information-dense enough to survey at a glance, structured enough to audit without friction, and calm enough to use under pressure without visual noise creating extra cognitive load. Every element earns its place by helping an engineer answer "which requests match, where do they go, what happens if nothing matches."

The interface is plain and inspectable by design. There is no decoration here that does not serve legibility. Forms, tables, and configuration views are the primary surfaces; their structure communicates the routing model directly. Engineers working in RustProxy should feel like they are reading a reliable operational ledger, not browsing a product dashboard.

**Key Characteristics:**
- Plain, legible, no decorative gradients or motion
- Information-dense tables and structured form layouts
- Border-based hierarchy rather than shadow-based elevation
- Monospace used for YAML editing and metric previews only
- Confidence through legible structure and explicit state

## 2. Colors

The palette is deliberately neutral and restrained. There is no brand color used as decoration; color is reserved for state and consequence.

### Neutral
- **Paper White** (#ffffff): Primary background, content surfaces.
- **Cool Gray Surface** (#f6f8fa): Muted fill for metric previews, code blocks, alternating table rows.
- **Warm Border** (#ddd): Card borders, form field borders, table header dividers. Slightly warm to avoid clinical coolness.
- **Subtle Border** (#eee): Table row dividers, internal separators. Lighter than #ddd to create visual hierarchy without noise.
- **Near Black** (#24292f): Primary text, headings, button text. Not pure black.
- **Muted Text** (#57606a): Secondary labels, helper text, muted actions.

### Semantic (derived from neutral, used sparingly)
- **Hover state on primary buttons**: transitions from #24292f to #57606a. No color shift elsewhere.
- **Error / Loading states**: handled via message text only in current implementation; no semantic color tokens yet.

### Named Rules
**The Ledger Discipline Rule.** Color is used for state and legibility, never decoration. The interface does not use gradients, saturated accents, or color fills that do not communicate information.

## 3. Typography

**Font Family:** System font stack for all UI: `-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif`.

**Monospace Font:** `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace` for YAML editor and Prometheus metric previews only.

**Character:** Functional system typography. No brand display font. The pairing is plain and disappears into the task.

### Hierarchy
- **Display** (600, 1.75rem, 1.25 line-height): Page-level headings (h1, section titles).
- **Body** (400, 1rem, 1.5 line-height): Default text, form labels, table content.
- **Label** (500, 0.875rem): Table column headers, field labels, secondary UI text.
- **Mono** (400, 0.875rem, 1.5 line-height): YAML editor textarea, metric output pre.

### Named Rules
**The Plain Ledger Rule.** Headings and labels use the same system font family as body text. No display serif, no emphasized typographic hierarchy beyond weight and size. The tool disappears.

## 4. Elevation

The interface is flat by default. Depth and visual hierarchy come from 1px borders and tonal surface shifts, not shadows. This is not a style choice in the abstract; it reflects how control-room interfaces stay legible under high-information conditions.

### Shadow Vocabulary
No shadows are currently in use. If a future pattern requires elevation (e.g. a dropdown or tooltip), use a very subtle shadow: `0 1px 3px rgba(0,0,0,0.12)` — ambient and minimal, never competing with the border-based hierarchy.

### Named Rules
**The Flat-By-Default Rule.** Surfaces are flat at rest. Cards, fieldsets, and form containers use 1px borders (`#ddd`) for separation. Shadows appear only if a component requires focus-overlap elevation (dropdown, popover, modal) and borders are insufficient.

## 5. Components

### Buttons
- **Shape:** 4px border radius.
- **Primary:** Background `#24292f`, text `#ffffff`, padding `0.5rem 1rem`. Used for main actions (Save, Submit).
- **Hover:** Background transitions to `#57606a`.
- **Ghost / Text-only:** No background, text `#24292f`, padding `0.5rem 1rem`. Used for Cancel, secondary actions, inline links.
- **Danger:** Not yet defined. Delete actions use the ghost/text button style currently.

### Inputs / Text Fields
- **Style:** 1px solid `#ddd` border, 4px border radius, padding `0.5rem`.
- **Focus:** No explicit focus ring in current implementation; relies on browser default.
- **Textarea (YAML):** Monospace font, full width, 24 rows. Background `#f6f8fa` for visual distinction from regular inputs.

### Cards / Containers
- **Corner Style:** 8px border radius (slightly rounder than inputs).
- **Background:** `#ffffff` (same as body background).
- **Border:** 1px solid `#ddd`.
- **Internal Padding:** 1rem.
- **Min-width:** 10rem for stat cards on Dashboard.

### Tables
- **Corner Style:** No rounded corners on the table itself.
- **Border-bottom** on `thead tr`: 1px solid `#ddd`. Used as the horizontal rule.
- **Cell padding:** `0.5rem`. Creates readable row height without excessive spacing.
- **Row dividers:** 1px solid `#eee` (lighter than header border to create a descending hierarchy).
- **Header font:** 500 weight, 0.875rem.

### Forms
- **Layout:** `display: grid; gap: 0.25rem` for label-to-input grouping, `display: grid; gap: 1rem` between form groups.
- **Fieldsets:** 1px solid `#ddd` border, 8px border radius, 1rem padding. Used for grouping related inputs (Conditions, Targets).
- **Inline form rows:** `display: flex; gap: 1rem; alignItems: center` for button groups and side-by-side actions.

### Navigation
- **Style:** Plain text links, `display: flex; gap: 1rem`. No sidebar, no active state styling in current implementation.
- **Margin-bottom:** 2rem from main content to separate from page content.

## 6. Do's and Don'ts

### Do:
- **Do** use 1px borders in `#ddd` for form fields, card containers, and table headers.
- **Do** use `#f6f8fa` as a muted background for code preview, metric previews, and YAML editor textarea.
- **Do** use the system font stack consistently for all UI text.
- **Do** use monospace font only for YAML editor and metrics display.
- **Do** use `8px` radius on cards and fieldsets, `4px` radius on inputs and buttons.
- **Do** use `0.5rem` padding for table cells and `1rem` for card internals.
- **Do** make tables and forms the primary layout vocabulary.

### Don't:
- **Don't** use decorative gradients anywhere. This interface should never have `background: linear-gradient(...)` or `background-clip: text`.
- **Don't** use shadow-based elevation. Cards and form containers are flat with border-based separation.
- **Don't** use large display fonts, flashy metrics, or hero-sized numbers in stat cards. Keep stat cards plain: the number, the label, the link.
- **Don't** use saturated accent colors as fills or backgrounds. The accent is `#24292f` (near-black) for text and primary buttons only.
- **Don't** use glassmorphism, frosted glass backgrounds, or backdrop-blur effects. This is not that kind of tool.
- **Don't** use terminal-native monospace styling as the primary visual language. Monospace is reserved for YAML and metrics.
- **Don't** use drag-and-drop or visual programming metaphors for rule configuration. Rules are structured forms.
- **Don't** use modals as the primary editing pattern. Form pages and inline editing are preferred.
- **Don't** use decorative motion or load animations. Product slop test: if it looks like a 2014 admin panel, the shadow is too heavy or the spacing is arbitrary.