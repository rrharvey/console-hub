// The console sidebar shows the balance.
// get-balance-due -> { kind: "ok", data: { balance (cents), pendingDebitBalanceCents } }
function balance({ due }, { money }) {
  const b = due.data;
  if (!b || b.balance == null) return null;
  const pending = Number(b.pendingDebitBalanceCents || 0);
  return { value: money(b.balance / 100), detail: pending > 0 ? `Pending charges: ${money(pending / 100)}` : undefined };
}
