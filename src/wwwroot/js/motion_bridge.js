/**
 * GeoViz Pro - Motion micro-interactions (native Web Animations API, no external deps)
 */

window.geoVizMotion = {
  // Animate view transitions (fade & slight slide/scale)
  animateViewIn: function (elementSelector) {
    const el = document.querySelector(elementSelector);
    if (!el) return;
    el.animate(
      [
        { opacity: 0, transform: 'scale(0.99) translateY(4px)' },
        { opacity: 1, transform: 'scale(1) translateY(0px)' }
      ],
      { duration: 220, easing: 'cubic-bezier(0.16, 1, 0.3, 1)', fill: 'forwards' }
    );
  },

  // Animate pulse of metric or calculation result badges
  pulseElement: function (selector) {
    const el = document.querySelector(selector);
    if (!el || typeof el.animate !== 'function') return;

    el.animate(
      [
        { transform: 'scale(1)', filter: 'brightness(1)' },
        { transform: 'scale(1.08)', filter: 'brightness(1.3)' },
        { transform: 'scale(1)', filter: 'brightness(1)' }
      ],
      { duration: 320, easing: 'cubic-bezier(0.175, 0.885, 0.32, 1.275)' }
    );
  },

  // Interactive hover/press micro-animation binding
  bindInteractiveFeedback: function () {
    document.querySelectorAll('.btn, .nav-tab-btn, .calc-tab-pill, .tool-card-item').forEach(btn => {
      if (btn.dataset.motionBound) return;
      btn.dataset.motionBound = 'true';

      btn.addEventListener('mousedown', () => {
        btn.style.transform = 'scale(0.97)';
        btn.style.transition = 'transform 0.08s ease';
      });

      const release = () => {
        btn.style.transform = '';
        btn.style.transition = 'transform 0.15s cubic-bezier(0.16, 1, 0.3, 1)';
      };

      btn.addEventListener('mouseup', release);
      btn.addEventListener('mouseleave', release);
    });
  }
};

// Automatically bind interactive feedback after DOM modifications
document.addEventListener('DOMContentLoaded', () => {
  window.geoVizMotion.bindInteractiveFeedback();
});
