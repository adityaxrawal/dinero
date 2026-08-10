/**
 * Turns raw merchant strings from bank feeds into readable names and icons.
 *
 * Payment networks deliver merchant descriptors in whatever form the acquiring
 * bank registered -- "ZOMATO MEDIA PVTLTD", "BHARTIAIRTELLTD", all-caps and
 * unspaced. Showing those verbatim makes a transaction list hard to scan, so
 * this module normalises them through three escalating stages:
 *
 *   1. Brand aliasing -- a substring match against known merchants collapses
 *      every descriptor variant to one canonical consumer-facing name.
 *   2. Suffix expansion -- corporate suffixes are spaced and abbreviated.
 *   3. Title casing -- applied only when the string is entirely uppercase, so
 *      a descriptor that already has deliberate casing is left untouched.
 *
 * The second export assigns each merchant a category icon and colour, matching
 * on either the assigned category or the merchant name so a transaction still
 * gets a sensible icon before it has been categorised.
 */
import React from "react";
import {
  Utensils,
  ShoppingBag,
  Smartphone,
  Zap,
  CreditCard,
  Car,
  Film,
  HelpCircle,
} from "lucide-react";

// Canonical name paired with the descriptor fragments that map onto it.
// Fragments are matched case-insensitively as substrings, which is what catches
// the padded and concatenated forms banks emit.
const BRAND_ALIASES: [name: string, fragments: string[]][] = [
  ["Zomato", ["zomato"]],
  ["Swiggy", ["swiggy"]],
  ["Blinkit", ["blinkit", "blinkmerc"]],
  ["Flipkart", ["flipkart"]],
  ["Amazon", ["amazon"]],
  ["Airtel", ["airtel", "bhartiairtel"]],
  ["Reliance Jio", ["jio"]],
  ["CRED", ["dreamplug", "cred"]],
  ["Uber", ["uber"]],
  ["Ola", ["ola"]],
  ["Netflix", ["netflix"]],
  ["Spotify", ["spotify"]],
  ["MakeMyTrip", ["make my trip", "makemytrip"]],
  ["BookMyShow", ["bookmyshow"]],
  ["Zepto", ["zepto"]],
];

// Corporate suffixes, applied in order. Each replacement is prefixed with a
// space because these arrive concatenated onto the name ("...PVTLTD"), and the
// longer patterns are listed before their substrings so PVTLTD is consumed
// before the bare LTD rule can match part of it.
const SUFFIX_EXPANSIONS: [pattern: RegExp, replacement: string][] = [
  [/PVTLTD/gi, " Pvt Ltd"],
  [/LTD/gi, " Ltd"],
  [/TECHNOLOGIES/gi, " Tech"],
  [/TECHNOLOGI/gi, " Tech"],
  [/SERVICES/gi, " Services"],
  [/PAYMENTS/gi, " Payments"],
  [/SOLUTIONS/gi, " Solutions"],
];

/** Lowercase everything, then capitalise each space-separated word. */
const toTitleCase = (value: string): string =>
  value
    .toLowerCase()
    .split(" ")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");

/**
 * Normalise a raw bank descriptor into a readable merchant name.
 *
 * Runs the three stages described at the top of this file, short-circuiting as
 * soon as a known brand is recognised.
 */
export function formatMerchantName(rawName: string): string {
  if (!rawName) return "Unknown Merchant";

  const trimmed = rawName.trim();
  const lower = trimmed.toLowerCase();

  // Stage 1. A brand hit wins outright -- the canonical name is returned
  // as-is, with no further cleanup, since it is already correctly cased.
  const brand = BRAND_ALIASES.find(([, fragments]) =>
    fragments.some((fragment) => lower.includes(fragment)),
  );
  if (brand) return brand[0];

  // Stage 2. Unknown merchant: expand corporate suffixes in declaration order.
  const cleaned = SUFFIX_EXPANSIONS.reduce(
    (name, [pattern, replacement]) => name.replace(pattern, replacement),
    trimmed,
  );

  // Stage 3. Title-case only if the descriptor is entirely uppercase. A string
  // with any lowercase already carries intentional casing ("iPhone", "eBay"),
  // and rewriting it would make it worse rather than better.
  return cleaned === cleaned.toUpperCase() ? toTitleCase(cleaned) : cleaned;
}

/** A ready-to-render icon element plus its background and text classes. */
interface CategoryVisual {
  icon: React.ReactNode;
  bgClass: string;
  textClass: string;
}

// Icon and colour per spending category. Each rule can be reached two ways --
// by category fragment or by merchant fragment -- so a transaction still gets a
// meaningful icon before categorisation has run. First match wins, making
// declaration order the tie-breaker for merchants that could fit two rules.
const CATEGORY_RULES: {
  categories: string[];
  merchants: string[];
  icon: typeof Utensils;
  bgClass: string;
  textClass: string;
}[] = [
  {
    categories: ["food", "dining"],
    merchants: ["zomato", "swiggy"],
    icon: Utensils,
    bgClass: "bg-amber-500/15 border-amber-500/20",
    textClass: "text-amber-800",
  },
  {
    categories: ["shop", "store"],
    merchants: ["flipkart", "amazon", "myntra"],
    icon: ShoppingBag,
    bgClass: "bg-purple-500/15 border-purple-500/20",
    textClass: "text-purple-800",
  },
  {
    categories: ["bill", "utility"],
    merchants: ["airtel", "jio", "electricity"],
    icon: Smartphone,
    bgClass: "bg-blue-500/15 border-blue-500/20",
    textClass: "text-blue-800",
  },
  {
    categories: ["grocer"],
    merchants: ["blinkit", "zepto", "instamart"],
    icon: Zap,
    bgClass: "bg-emerald-500/15 border-emerald-500/20",
    textClass: "text-emerald-800",
  },
  {
    categories: ["finance", "card"],
    merchants: ["cred", "dreamplug"],
    icon: CreditCard,
    bgClass: "bg-indigo-500/15 border-indigo-500/20",
    textClass: "text-indigo-800",
  },
  {
    categories: ["travel", "cab"],
    merchants: ["uber", "ola"],
    icon: Car,
    bgClass: "bg-sky-500/15 border-sky-500/20",
    textClass: "text-sky-800",
  },
  {
    categories: ["entertain"],
    merchants: ["netflix", "bookmyshow", "spotify"],
    icon: Film,
    bgClass: "bg-rose-500/15 border-rose-500/20",
    textClass: "text-rose-800",
  },
];

/**
 * Pick the icon and colour classes for a transaction's merchant.
 *
 * Both inputs are optional; either alone is enough to find a rule, and when
 * neither matches the caller still receives a complete visual rather than null,
 * so render sites never need a fallback branch of their own.
 */
export function getMerchantCategoryVisuals(
  category?: string,
  merchantName?: string,
): CategoryVisual {
  // Coerced to lowercase strings up front so the fragment tests below are
  // uniform and null-safe.
  const cat = (category || "").toLowerCase();
  const name = (merchantName || "").toLowerCase();

  const rule = CATEGORY_RULES.find(
    (candidate) =>
      candidate.categories.some((fragment) => cat.includes(fragment)) ||
      candidate.merchants.some((fragment) => name.includes(fragment)),
  );

  // Neutral brand-coloured fallback for uncategorised merchants -- deliberately
  // not one of the category colours, so "unknown" is never mistaken for a real
  // classification.
  if (!rule) {
    return {
      icon: React.createElement(HelpCircle, { className: "w-4 h-4" }),
      bgClass: "bg-[#064E3B]/10 border-[#064E3B]/15",
      textClass: "text-[#064E3B]",
    };
  }

  return {
    icon: React.createElement(rule.icon, { className: "w-4 h-4" }),
    bgClass: rule.bgClass,
    textClass: rule.textClass,
  };
}
