export interface Summary {
  count: number;
  last: number;
  median: number;
  p95: number;
}

export const NO_SAMPLES: Summary = { count: 0, last: 0, median: 0, p95: 0 };

/** A bounded list of timings with nearest-rank percentiles. */
export class Samples {
  #values: number[] = [];
  #keep: number;

  constructor(keep = 500) {
    this.#keep = keep;
  }

  add(value: number): void {
    if (this.#values.length === this.#keep) this.#values.shift();
    this.#values.push(value);
  }

  summary(): Summary {
    const count = this.#values.length;
    if (count === 0) return NO_SAMPLES;
    const sorted = [...this.#values].sort((a, b) => a - b);
    const at = (q: number) => sorted[Math.min(count, Math.max(1, Math.ceil(count * q))) - 1];
    return { count, last: this.#values[count - 1], median: at(0.5), p95: at(0.95) };
  }
}
