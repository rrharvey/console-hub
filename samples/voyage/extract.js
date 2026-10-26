// api/balance/<org> -> { data: { balance: { balance: "5", autoRechargeEnabled } } }
/** @param {Captures} captures @param {Helpers} helpers @returns {MetricResult | null} */
function balance({ balance: res }, { money }) {
  const b = res.data?.balance;
  if (!b || b.balance == null) return null;
  return { value: money(b.balance), detail: b.autoRechargeEnabled ? "Auto recharge on" : "Auto recharge off" };
}
