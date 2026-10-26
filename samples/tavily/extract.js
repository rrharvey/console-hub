// api/account -> { usage, limit, addon_limit, plan_display_name, paygo* }
// Tavily sells credits, not a dollar balance, so show credits remaining this cycle.
// With pay as you go on, the console adds paygo_usage/paygo_limit (credits) to the plan's.
/** @param {Captures} captures @param {Helpers} helpers @returns {MetricResult | null} */
function balance({ account: a }, { money, number }) {
  if (a.limit == null) return null;
  const planLimit = Number(a.limit) + Number(a.addon_limit || 0);
  const planUsed = Number(a.usage || 0);
  const paygo = a.paygo ? { used: Number(a.paygo_usage || 0), limit: Number(a.paygo_limit || 0) } : null;
  const used = planUsed + (paygo ? paygo.used : 0);
  const limit = planLimit + (paygo ? paygo.limit : 0);
  const detail = [`${number(used)} / ${number(limit)} credits used`];
  detail.push(`Plan (${a.plan_display_name || a.current_plan}): ${number(planUsed)} / ${number(planLimit)}`);
  if (paygo) {
    let line = `Pay as you go: ${number(paygo.used)} / ${number(paygo.limit)}`;
    if (a.paygo_cpc != null) line += ` (${money(paygo.used * Number(a.paygo_cpc))} at $${Number(a.paygo_cpc)}/credit)`;
    detail.push(line);
  }
  if (a.reset_cycle) detail.push(`Resets ${a.reset_cycle}`);
  return { value: `${number(Math.max(0, limit - used))} credits`, detail: detail.join("\n") };
}
