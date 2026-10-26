// Types for a provider's extract.js, for editors (see PLUGINS.md). The app doesn't load
// this file. extract.js runs in a sandbox with plain JavaScript only: no DOM, no network,
// no Intl, no timers.

/** Formatting helpers, the second argument of every metric function. */
interface Helpers {
  /**
   * Format an amount like Intl.NumberFormat("en-US", { style: "currency", currency }):
   * `money(1234.5)` is "$1,234.50", `money("9", "EUR")` is "€9.00". Numeric strings work.
   */
  money(value: number | string, currency?: string): string;
  /** Group a number like `n.toLocaleString("en-US")`, with at most 3 decimals: "117,500". */
  number(value: number | string): string;
}

/** The captured JSON response bodies, by capture name. A capture that isn't required may be missing. */
type Captures = Record<string, any>;

/** What a metric function returns. Return null instead while there's no balance in the responses. */
interface MetricResult {
  /** Shown in the sidebar, 1 to 40 characters: "$12.34", "1,200 credits". */
  value: string;
  /** Shown in the tooltip, up to 1000 characters. Separate lines with "\n". */
  detail?: string;
}
