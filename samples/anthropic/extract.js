// prepaid/credits -> { amount (minor units), currency, balance.credits: { amount_minor, exponent } }
/** @param {Captures} captures @param {Helpers} helpers @returns {MetricResult | null} */
function balance({ credits }, { money }) {
  const minor = credits.balance?.credits?.amount_minor ?? credits.amount;
  const exp = credits.balance?.credits?.exponent ?? 2;
  if (minor == null) return null;
  return { value: money(minor / 10 ** exp, credits.currency || "USD") };
}
