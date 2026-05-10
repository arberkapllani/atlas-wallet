/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{html,svelte,ts}'],
  darkMode: ['class', '[data-theme="dark"]'],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          'Inter',
          'ui-sans-serif',
          'system-ui',
          '-apple-system',
          'Segoe UI',
          'Roboto',
          'sans-serif'
        ],
        display: ['"Space Grotesk"', 'Inter', 'system-ui', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'monospace']
      },
      colors: {
        // Background tiers — driven by CSS variables so light/dark swap them.
        bg: {
          DEFAULT: 'rgb(var(--bg) / <alpha-value>)',
          subtle: 'rgb(var(--bg-subtle) / <alpha-value>)',
          elevated: 'rgb(var(--bg-elevated) / <alpha-value>)'
        },
        border: {
          DEFAULT: 'rgb(var(--border) / <alpha-value>)',
          subtle: 'rgb(var(--border-subtle) / <alpha-value>)'
        },
        fg: {
          DEFAULT: 'rgb(var(--fg) / <alpha-value>)',
          muted: 'rgb(var(--fg-muted) / <alpha-value>)',
          subtle: 'rgb(var(--fg-subtle) / <alpha-value>)'
        },
        // Brand: deep, confident green.
        brand: {
          50: '#ECFDF3',
          100: '#D1FADF',
          200: '#A6F4C5',
          300: '#6CE9A6',
          400: '#32D583',
          500: '#12B76A',
          600: '#059669',
          700: '#047857',
          800: '#065F46',
          900: '#064E3B',
          DEFAULT: '#0F9D58'
        },
        // Warm supporting tone.
        earth: {
          50: '#F7F1EA',
          100: '#EADBC4',
          200: '#D9BC97',
          300: '#C39A6A',
          400: '#A67946',
          500: '#8B5E3C',
          600: '#6F4A2E',
          700: '#5A3B22',
          800: '#3F2A1A',
          900: '#2A1C12',
          DEFAULT: '#8B5E3C'
        },
        // Highlight accent: signature orange.
        accent: {
          50: '#FFF1E1',
          100: '#FFE0BF',
          200: '#FCC58D',
          300: '#FAA85F',
          400: '#F58727',
          500: '#E5751B',
          600: '#C66012',
          700: '#9D4B0E',
          800: '#7B3A0B',
          900: '#562809',
          DEFAULT: '#F58727',
          hover: '#E5751B',
          ring: 'rgba(245, 135, 39, 0.4)'
        },
        success: '#10B981',
        danger: '#EF4444',
        warning: '#F59E0B'
      },
      backgroundImage: {
        'brand-gradient': 'linear-gradient(135deg, #0F9D58 0%, #12B76A 50%, #F58727 100%)',
        'brand-soft':
          'radial-gradient(80% 60% at 50% 0%, rgba(16, 185, 129, 0.18) 0%, transparent 70%)',
        'brand-glow': 'radial-gradient(ellipse at top, rgba(15, 157, 88, 0.18), transparent 60%)',
        'mesh-dark':
          'radial-gradient(40% 60% at 100% 0%, rgba(245, 135, 39, 0.10), transparent 70%), radial-gradient(50% 50% at 0% 100%, rgba(15, 157, 88, 0.10), transparent 70%)',
        'mesh-light':
          'radial-gradient(40% 60% at 100% 0%, rgba(245, 135, 39, 0.16), transparent 70%), radial-gradient(50% 50% at 0% 100%, rgba(15, 157, 88, 0.14), transparent 70%)'
      },
      boxShadow: {
        card: '0 1px 0 rgba(255, 255, 255, 0.04) inset, 0 8px 32px rgba(0, 0, 0, 0.18)',
        soft: '0 4px 14px rgba(0, 0, 0, 0.06), 0 1px 2px rgba(0, 0, 0, 0.04)',
        glow: '0 0 0 1px rgba(15, 157, 88, 0.4), 0 12px 32px rgba(15, 157, 88, 0.25)',
        accentGlow: '0 0 0 1px rgba(245, 135, 39, 0.4), 0 12px 32px rgba(245, 135, 39, 0.25)'
      },
      borderRadius: {
        xl: '14px',
        '2xl': '20px',
        '3xl': '28px'
      },
      transitionTimingFunction: {
        smooth: 'cubic-bezier(0.22, 1, 0.36, 1)'
      },
      keyframes: {
        'fade-in-up': {
          '0%': { opacity: '0', transform: 'translateY(8px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' }
        },
        shimmer: {
          '0%': { backgroundPosition: '-200% 0' },
          '100%': { backgroundPosition: '200% 0' }
        }
      },
      animation: {
        'fade-in-up': 'fade-in-up 280ms cubic-bezier(0.22, 1, 0.36, 1) both',
        shimmer: 'shimmer 1.6s linear infinite'
      }
    }
  },
  plugins: []
};
