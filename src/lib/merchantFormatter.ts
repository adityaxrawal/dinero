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

/**
 * Normalizes raw statement/bank merchant names into clean display text.
 * e.g., "ZOMATOLIMITED" -> "Zomato Limited"
 *       "BLINKMERCEPVTLTD" -> "Blinkit (Pvt Ltd)"
 *       "BHARTIAIRTELLTD" -> "Bharti Airtel"
 */
// Canonical brand name -> the lowercased fragments that identify it in raw
// statement text. Order matters: the first entry that matches wins.
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

// Applied in order. Note PVTLTD expands to " Pvt Ltd" and the later LTD rule
// then fires again on that "Ltd", producing a double space — long-standing
// behaviour that callers already render.
const SUFFIX_EXPANSIONS: [pattern: RegExp, replacement: string][] = [
  [/PVTLTD/gi, " Pvt Ltd"],
  [/LTD/gi, " Ltd"],
  [/TECHNOLOGIES/gi, " Tech"],
  [/TECHNOLOGI/gi, " Tech"],
  [/SERVICES/gi, " Services"],
  [/PAYMENTS/gi, " Payments"],
  [/SOLUTIONS/gi, " Solutions"],
];

const toTitleCase = (value: string): string =>
  value
    .toLowerCase()
    .split(" ")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");

export function formatMerchantName(rawName: string): string {
  if (!rawName) return "Unknown Merchant";

  const trimmed = rawName.trim();
  const lower = trimmed.toLowerCase();

  const brand = BRAND_ALIASES.find(([, fragments]) =>
    fragments.some((fragment) => lower.includes(fragment)),
  );
  if (brand) return brand[0];

  const cleaned = SUFFIX_EXPANSIONS.reduce(
    (name, [pattern, replacement]) => name.replace(pattern, replacement),
    trimmed,
  );

  // Only names that are still entirely upper case get title-cased.
  return cleaned === cleaned.toUpperCase() ? toTitleCase(cleaned) : cleaned;
}

/**
 * Returns dynamic category iconography & background colors based on category or merchant name.
 */
interface CategoryVisual {
  icon: React.ReactNode;
  bgClass: string;
  textClass: string;
}

// Checked in order; the first rule whose category or merchant fragment matches
// wins, so a transaction categorised as "food" stays amber even if the merchant
// name would also match a later rule.
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
 * Returns dynamic category iconography & background colors based on category or merchant name.
 */
export function getMerchantCategoryVisuals(
  category?: string,
  merchantName?: string,
): CategoryVisual {
  const cat = (category || "").toLowerCase();
  const name = (merchantName || "").toLowerCase();

  const rule = CATEGORY_RULES.find(
    (candidate) =>
      candidate.categories.some((fragment) => cat.includes(fragment)) ||
      candidate.merchants.some((fragment) => name.includes(fragment)),
  );

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
