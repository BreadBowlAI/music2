// The graph's nodes, for the suites (taste.md DV-12, DV-13): each starts collapsed, and a suite
// whose value is hidden there expands its node first. Expanding moves the other nodes aside over
// 280 ms; `still` waits until they stop (the graph's `data-moving`), so a box read after it is
// where the node stays.

/** Until the graph's nodes stop moving aside, or `ms` pass. */
export async function still(page, ms = 3000) {
  const t = Date.now();
  while (Date.now() - t < ms) {
    if (!(await page.$("[data-moving]"))) return true;
    await new Promise((r) => setTimeout(r, 40));
  }
  return false;
}

/**
 * Expand every node `sel` names (a `[data-node]` selector), each one not expanded yet, and wait
 * until the graph stops moving. True when each is expanded.
 */
export async function expand(page, sel) {
  for (let i = 0; i < 20; i++) {
    const button = await page.$(`${sel}[data-size="collapsed"] [data-expand]`);
    if (!button) break;
    await button.click();
    await still(page);
  }
  await still(page);
  return !(await page.$(`${sel}[data-size="collapsed"]`));
}
