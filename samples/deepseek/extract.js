// get_user_summary -> { data.biz_data: { normal_wallets[], bonus_wallets[] } }
/** @param {Captures} captures @param {Helpers} helpers @returns {MetricResult | null} */
function balance({ summary }, { money }) {
  const d = summary.data?.biz_data;
  if (!d || !d.normal_wallets) return null;
  const sum = (ws, cur) => (ws || []).filter((w) => w.currency === cur).reduce((a, w) => a + Number(w.balance || 0), 0);
  const cur = d.normal_wallets[0]?.currency || "USD";
  const paid = sum(d.normal_wallets, cur);
  const bonus = sum(d.bonus_wallets, cur);
  const detail = [`Topped up: ${money(paid, cur)}`, `Granted: ${money(bonus, cur)}`];
  return { value: money(paid + bonus, cur), detail: detail.join("\n") };
}
