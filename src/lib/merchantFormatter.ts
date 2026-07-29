import React from "react";
import {
  Utensils,
  ShoppingBag,
  Smartphone,
  Zap,
  CreditCard,
  Car,
  Film,
  HeartPulse,
  Briefcase,
  HelpCircle,
  ArrowUpRight,
  ArrowDownRight,
} from "lucide-react";

interface FormattedMerchantInfo {
  cleanName: string;
  categoryIcon: React.ReactNode;
  iconBg: string;
  iconColor: string;
}

/**
 * Normalizes raw statement/bank merchant names into clean display text.
 * e.g., "ZOMATOLIMITED" -> "Zomato Limited"
 *       "BLINKMERCEPVTLTD" -> "Blinkit (Pvt Ltd)"
 *       "BHARTIAIRTELLTD" -> "Bharti Airtel"
 */
export function formatMerchantName(rawName: string): string {
  if (!rawName) return "Unknown Merchant";

  let cleaned = rawName.trim();

  // Known brand replacements
  const lower = cleaned.toLowerCase();
  if (lower.includes("zomato")) return "Zomato";
  if (lower.includes("swiggy")) return "Swiggy";
  if (lower.includes("blinkit") || lower.includes("blinkmerc"))
    return "Blinkit";
  if (lower.includes("flipkart")) return "Flipkart";
  if (lower.includes("amazon")) return "Amazon";
  if (lower.includes("airtel") || lower.includes("bhartiairtel"))
    return "Airtel";
  if (lower.includes("jio")) return "Reliance Jio";
  if (lower.includes("dreamplug") || lower.includes("cred")) return "CRED";
  if (lower.includes("uber")) return "Uber";
  if (lower.includes("ola")) return "Ola";
  if (lower.includes("netflix")) return "Netflix";
  if (lower.includes("spotify")) return "Spotify";
  if (lower.includes("make my trip") || lower.includes("makemytrip"))
    return "MakeMyTrip";
  if (lower.includes("bookmyshow")) return "BookMyShow";
  if (lower.includes("zepto")) return "Zepto";

  // Common corporate suffix cleanups
  cleaned = cleaned
    .replace(/PVTLTD/gi, " Pvt Ltd")
    .replace(/LTD/gi, " Ltd")
    .replace(/TECHNOLOGIES/gi, " Tech")
    .replace(/TECHNOLOGI/gi, " Tech")
    .replace(/SERVICES/gi, " Services")
    .replace(/PAYMENTS/gi, " Payments")
    .replace(/SOLUTIONS/gi, " Solutions");

  // Convert ALL CAPS to Title Case
  if (cleaned === cleaned.toUpperCase()) {
    cleaned = cleaned
      .toLowerCase()
      .split(" ")
      .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
      .join(" ");
  }

  return cleaned;
}

/**
 * Returns dynamic category iconography & background colors based on category or merchant name.
 */
export function getMerchantCategoryVisuals(
  category?: string,
  merchantName?: string,
): {
  icon: React.ReactNode;
  bgClass: string;
  textClass: string;
} {
  const cat = (category || "").toLowerCase();
  const name = (merchantName || "").toLowerCase();

  // Food & Dining
  if (
    cat.includes("food") ||
    cat.includes("dining") ||
    name.includes("zomato") ||
    name.includes("swiggy")
  ) {
    return {
      icon: React.createElement(Utensils, { className: "w-4 h-4" }),
      bgClass: "bg-amber-500/15 border-amber-500/20",
      textClass: "text-amber-800",
    };
  }

  // Shopping / E-commerce
  if (
    cat.includes("shop") ||
    cat.includes("store") ||
    name.includes("flipkart") ||
    name.includes("amazon") ||
    name.includes("myntra")
  ) {
    return {
      icon: React.createElement(ShoppingBag, { className: "w-4 h-4" }),
      bgClass: "bg-purple-500/15 border-purple-500/20",
      textClass: "text-purple-800",
    };
  }

  // Bills / Telecom / Utilities
  if (
    cat.includes("bill") ||
    cat.includes("utility") ||
    name.includes("airtel") ||
    name.includes("jio") ||
    name.includes("electricity")
  ) {
    return {
      icon: React.createElement(Smartphone, { className: "w-4 h-4" }),
      bgClass: "bg-blue-500/15 border-blue-500/20",
      textClass: "text-blue-800",
    };
  }

  // Grocery / Quick Commerce
  if (
    cat.includes("grocer") ||
    name.includes("blinkit") ||
    name.includes("zepto") ||
    name.includes("instamart")
  ) {
    return {
      icon: React.createElement(Zap, { className: "w-4 h-4" }),
      bgClass: "bg-emerald-500/15 border-emerald-500/20",
      textClass: "text-emerald-800",
    };
  }

  // Financial / Credit / Cred
  if (
    cat.includes("finance") ||
    cat.includes("card") ||
    name.includes("cred") ||
    name.includes("dreamplug")
  ) {
    return {
      icon: React.createElement(CreditCard, { className: "w-4 h-4" }),
      bgClass: "bg-indigo-500/15 border-indigo-500/20",
      textClass: "text-indigo-800",
    };
  }

  // Travel / Commute
  if (
    cat.includes("travel") ||
    cat.includes("cab") ||
    name.includes("uber") ||
    name.includes("ola")
  ) {
    return {
      icon: React.createElement(Car, { className: "w-4 h-4" }),
      bgClass: "bg-sky-500/15 border-sky-500/20",
      textClass: "text-sky-800",
    };
  }

  // Entertainment
  if (
    cat.includes("entertain") ||
    name.includes("netflix") ||
    name.includes("bookmyshow") ||
    name.includes("spotify")
  ) {
    return {
      icon: React.createElement(Film, { className: "w-4 h-4" }),
      bgClass: "bg-rose-500/15 border-rose-500/20",
      textClass: "text-rose-800",
    };
  }

  // Default Fallback
  return {
    icon: React.createElement(HelpCircle, { className: "w-4 h-4" }),
    bgClass: "bg-[#064E3B]/10 border-[#064E3B]/15",
    textClass: "text-[#064E3B]",
  };
}
