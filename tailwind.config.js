/**
 * Tailwind design-system configuration for the Dinero UI.
 *
 * The colour palette is split into two deliberately different halves. The first
 * group (background, card, primary, destructive and friends) resolves through
 * `hsl(var(--token))` CSS custom properties rather than literal values, which is
 * what lets the shadcn/Radix primitives be re-themed at runtime by swapping the
 * variables defined in the global stylesheet. The second group (emerald-ink,
 * champagne, and the success/warning/danger trio) are fixed brand hex values
 * that must stay constant no matter which theme is active.
 *
 * Everything sits under `extend`, so Tailwind's stock scales remain available;
 * these entries add to the defaults rather than replacing them. The `spacing`
 * and `fontSize` blocks intentionally list only the steps this app actually
 * uses, which keeps the generated stylesheet small and the visual rhythm
 * consistent.
 */
export default {
  // Dark mode toggles via a `dark` class on an ancestor element, not the OS
  // media query -- the app drives it explicitly rather than following the system.
  darkMode: ['class'],

  // Files Tailwind scans to decide which utilities to emit. Anything outside
  // this glob gets its classes stripped from the production build.
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        card: {
          DEFAULT: 'hsl(var(--card))',
          foreground: 'hsl(var(--card-foreground))',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover))',
          foreground: 'hsl(var(--popover-foreground))',
        },
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary))',
          foreground: 'hsl(var(--secondary-foreground))',
        },
        muted: {
          DEFAULT: 'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent))',
          foreground: 'hsl(var(--accent-foreground))',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))',
        },
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',

        // Fixed brand colours below this line -- these are literal values on
        // purpose, so they do not shift when the themeable tokens above change.
        'emerald-ink': {
          DEFAULT: '#064E3B',
          hover: '#053d2f',
          light: '#0a6e53',
          subtle: 'rgba(6,78,59,0.07)',
        },
        champagne: {
          DEFAULT: '#F8E7C9',
          dark: '#f0d4a8',
          deeper: '#e8c888',
          card: '#fdf6ed',
        },

        // Reserved status roles. These carry meaning in the UI and are never
        // reused as arbitrary chart or decoration colours.
        success: '#10b981',
        warning: '#f59e0b',
        danger: '#ef4444',
      },

      fontFamily: {
        sans: ['"Inter"', '-apple-system', 'BlinkMacSystemFont', '"Segoe UI"', 'sans-serif'],
      },

      // Small/medium/large derive from a single --radius variable so the whole
      // corner treatment can be retuned from one place; xl and 2xl are fixed
      // sizes used by larger surfaces such as modals and cards.
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
        xl: '12px',
        '2xl': '16px',
      },

      fontSize: {
        xs: ['0.75rem', { lineHeight: '1rem' }],
        sm: ['0.875rem', { lineHeight: '1.25rem' }],
        base: ['1rem', { lineHeight: '1.5rem' }],
        lg: ['1.125rem', { lineHeight: '1.75rem' }],
        xl: ['1.25rem', { lineHeight: '1.75rem' }],
        '2xl': ['1.5rem', { lineHeight: '2rem' }],
        '3xl': ['1.875rem', { lineHeight: '2.25rem' }],
      },

      spacing: {
        1: '0.25rem',
        2: '0.5rem',
        3: '0.75rem',
        4: '1rem',
        6: '1.5rem',
        8: '2rem',
        12: '3rem',
      },

      // Keyframes are declared here and bound to named utilities in the
      // `animation` block below. The accordion pair reads its target height
      // from a Radix-provided variable, since the collapsed height is only
      // known at runtime.
      keyframes: {
        'accordion-down': {
          from: { height: '0' },
          to: { height: 'var(--radix-accordion-content-height)' },
        },
        'accordion-up': {
          from: { height: 'var(--radix-accordion-content-height)' },
          to: { height: '0' },
        },
        'skeleton-shimmer': {
          '0%': { backgroundPosition: '200% 0' },
          '100%': { backgroundPosition: '-200% 0' },
        },
        'fade-in': {
          from: { opacity: '0', transform: 'translateY(4px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'slide-in-left': {
          from: { opacity: '0', transform: 'translateX(-8px)' },
          to: { opacity: '1', transform: 'translateX(0)' },
        },
      },

      animation: {
        'accordion-down': 'accordion-down 0.2s ease-out',
        'accordion-up': 'accordion-up 0.2s ease-out',
        skeleton: 'skeleton-shimmer 1.4s ease-in-out infinite',
        'fade-in': 'fade-in 0.2s ease-out',
        'slide-in-left': 'slide-in-left 0.2s ease-out',
      },

      // Shadows are tinted with the emerald brand colour rather than neutral
      // black, so elevation reads as part of the palette. `emerald` is a focus
      // ring rather than a drop shadow -- two stacked spreads, no blur.
      boxShadow: {
        card: '0 1px 3px rgba(6,78,59,0.06), 0 1px 2px rgba(6,78,59,0.04)',
        'card-hover': '0 4px 12px rgba(6,78,59,0.10), 0 2px 4px rgba(6,78,59,0.06)',
        emerald: '0 0 0 2px rgba(6,78,59,0.45), 0 0 0 4px rgba(6,78,59,0.12)',
      },
    },
  },
  plugins: [],
};
