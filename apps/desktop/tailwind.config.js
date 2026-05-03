/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{html,svelte,ts}'],
  theme: {
    extend: {
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'ui-monospace', 'monospace']
      },
      colors: {
        // Dark, premium palette inspired by Exodus.
        bg: {
          DEFAULT: '#0b0d14',
          subtle: '#10131c',
          elevated: '#161a25'
        },
        border: {
          DEFAULT: '#212635',
          subtle: '#1a1f2c'
        },
        fg: {
          DEFAULT: '#e7eaf3',
          muted: '#9aa3b8',
          subtle: '#5d6884'
        },
        accent: {
          DEFAULT: '#7c5cff',
          hover: '#8b6dff',
          ring: 'rgba(124, 92, 255, 0.4)',
          cyan: '#22d3ee',
          gradientFrom: '#22d3ee',
          gradientTo: '#7c5cff'
        },
        success: '#22c55e',
        danger: '#ef4444',
        warning: '#f59e0b'
      },
      backgroundImage: {
        'brand-gradient':
          'linear-gradient(135deg, #22d3ee 0%, #7c5cff 60%, #c084fc 100%)',
        'brand-glow':
          'radial-gradient(ellipse at top, rgba(124,92,255,0.18), transparent 60%)'
      },
      boxShadow: {
        'card': '0 1px 0 rgba(255,255,255,0.04) inset, 0 8px 32px rgba(0,0,0,0.45)',
        'glow': '0 0 0 1px rgba(124, 92, 255, 0.4), 0 8px 32px rgba(124, 92, 255, 0.15)'
      },
      borderRadius: {
        xl: '14px',
        '2xl': '20px'
      }
    }
  },
  plugins: []
};
