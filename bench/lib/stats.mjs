/**
 * Basic sample statistics for benchmark results.
 */

export function percentile(sorted, p) {
	if (sorted.length === 0) return NaN;
	const idx = Math.min(
		sorted.length - 1,
		Math.max(0, Math.ceil((p / 100) * sorted.length) - 1),
	);
	return sorted[idx];
}

export function summarise(samples) {
	const sorted = [...samples].sort((a, b) => a - b);
	const n = sorted.length;
	if (n === 0) {
		return { n: 0 };
	}
	const sum = sorted.reduce((acc, v) => acc + v, 0);
	const mean = sum / n;
	const variance =
		n > 1
			? sorted.reduce((acc, v) => acc + (v - mean) ** 2, 0) / (n - 1)
			: 0;
	return {
		n,
		min: sorted[0],
		max: sorted[n - 1],
		mean,
		stddev: Math.sqrt(variance),
		p50: percentile(sorted, 50),
		p90: percentile(sorted, 90),
		p99: percentile(sorted, 99),
	};
}

export function fmtMs(value) {
	if (!Number.isFinite(value)) return "-";
	if (value >= 100) return value.toFixed(0);
	if (value >= 10) return value.toFixed(1);
	return value.toFixed(2);
}
