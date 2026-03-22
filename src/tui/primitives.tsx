/**
 * Typed wrappers for OpenTUI <span> and <b> elements.
 *
 * The @opentui/solid types for SpanProps don't expose fg/bg (the first
 * generic to ComponentProps is {}), but the underlying TextNodeRenderable
 * accepts them at runtime through TextNodeOptions.  These thin wrappers
 * give us proper TypeScript support without patching node_modules.
 */
import type { RGBA } from "@opentui/core";
import type { JSX } from "@opentui/solid/jsx-runtime";

interface TextNodeStyleProps {
  fg?: string | RGBA;
  bg?: string | RGBA;
  children?: JSX.Element;
}

export function Span(props: TextNodeStyleProps): JSX.Element {
  return <span {...(props as any)} />;
}

export function Bold(props: TextNodeStyleProps): JSX.Element {
  return <b {...(props as any)} />;
}
