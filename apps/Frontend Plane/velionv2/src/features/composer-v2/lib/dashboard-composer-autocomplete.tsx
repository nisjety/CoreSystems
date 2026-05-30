import { Calendar } from "lucide-react";
import { dayEntries } from "@/features/composer-v2/lib/dashboard-composer-options";
import type { AutocompleteItem, TriggerContext } from "@/features/composer-v2/lib/dashboard-composer-types";

export function getUpcomingDates(dayIndex: number): AutocompleteItem[] {
  const items: AutocompleteItem[] = [];
  const date = new Date();

  while (items.length < 2) {
    date.setDate(date.getDate() + 1);

    if (date.getDay() === dayIndex) {
      const label = date.toLocaleDateString("en-US", {
        weekday: "short",
        month: "short",
        day: "numeric",
      });
      items.push({
        id: `date-${label}`,
        icon: <Calendar className="size-4" />,
        label,
        meta: "date",
      });
    }
  }

  return items;
}

export function detectTrigger(text: string, position: number): TriggerContext | null {
  const before = text.slice(0, position);
  const personMatch = before.match(/@(\w*)$/);

  if (personMatch) {
    return {
      type: "person",
      query: personMatch[1].toLowerCase(),
      start: position - personMatch[0].length,
      rawLen: personMatch[0].length,
    };
  }

  const slashMatch = before.match(/\/(\w*)$/);

  if (slashMatch) {
    return {
      type: "slash",
      query: slashMatch[1].toLowerCase(),
      start: position - slashMatch[0].length,
      rawLen: slashMatch[0].length,
    };
  }

  const wordMatch = before.match(/\b([A-Za-z]{3,})$/);

  if (!wordMatch) {
    return null;
  }

  const lower = wordMatch[1].toLowerCase();
  const day = dayEntries.find((entry) => entry.name.toLowerCase().startsWith(lower));

  if (!day) {
    return null;
  }

  return {
    type: "date",
    dayIndex: day.dayIndex,
    start: position - wordMatch[1].length,
    rawLen: wordMatch[1].length,
  };
}
