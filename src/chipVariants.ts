export function chipClass(variant: string | undefined, base = 'chip'): string {
  const v = variant ?? 'neutral';
  return `${base} chip-${v}`;
}

export function kindChipClass(kind: 'issue' | 'pr', draft: boolean): string {
  if (draft) {
    return chipClass('draft', 'chip');
  }
  return kind === 'pr' ? chipClass('pr', 'chip') : chipClass('issue', 'chip');
}
