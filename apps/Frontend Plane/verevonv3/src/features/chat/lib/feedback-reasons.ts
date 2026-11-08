/**
 * The reasons a person can give for a thumbs-down on an answer. One list for every
 * surface that rates an answer, so a chat turn and a run's result offer the same
 * choices. The chosen label is sent as the rating's note (see `submitFeedback`).
 */
export function feedbackReasonLabels(i18n: { tr: (noText: string, enText: string) => string }): string[] {
  return [
    i18n.tr('Unøyaktig', 'Inaccurate'),
    i18n.tr('Ikke relevant', 'Not relevant'),
    i18n.tr('Ufullstendig', 'Incomplete'),
    i18n.tr('Skadelig', 'Harmful'),
  ]
}
