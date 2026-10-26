// The console sidebar shows the balance. Loading /keys makes a Next.js server call
// whose response rows include { ok, data: { balanceCents } }.
function balance({ keys }, { money }) {
  const cents = Array.isArray(keys) ? keys.find((r) => r?.data?.balanceCents != null)?.data.balanceCents : null;
  return cents == null ? null : { value: money(cents / 100) };
}
