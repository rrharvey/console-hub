// credit_grants -> { total_available, total_granted, total_used }; wallet -> { balance }
/** @param {Captures} captures @param {Helpers} helpers @returns {MetricResult | null} */
function balance({ grants, wallet }, { money }) {
  if (grants.total_available == null) return null;
  const detail = [`${money(grants.total_used)} used of ${money(grants.total_granted)} granted`];
  if (wallet && Number(wallet.balance) > 0) detail.push(`Wallet: ${money(wallet.balance)}`);
  return { value: money(grants.total_available), detail: detail.join("\n") };
}
