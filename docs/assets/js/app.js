/**
 * OxideGram Documentation Interactive Features
 * - Theme Switcher (Dark / Light) with persistence
 * - Code Block Copy Buttons
 * - Dynamic In-Page Table of Contents & ScrollSpy
 * - Quick Search / Heading Filter & Keyboard Shortcuts
 * - Mobile Sidebar Drawer
 */

document.addEventListener('DOMContentLoaded', () => {
  initThemeToggle();
  initCodeCopyButtons();
  initInPageToc();
  initQuickSearch();
  initMobileMenu();
  initBackToTop();
});

/* --------------------------------------------------------------------------
   1. Theme Switcher
   -------------------------------------------------------------------------- */
function initThemeToggle() {
  const toggleBtn = document.getElementById('themeToggle');
  if (!toggleBtn) return;

  toggleBtn.addEventListener('click', () => {
    const currentTheme = document.documentElement.getAttribute('data-theme') || 'dark';
    const nextTheme = currentTheme === 'dark' ? 'light' : 'dark';

    document.documentElement.setAttribute('data-theme', nextTheme);
    localStorage.setItem('oxidegram-theme', nextTheme);
  });
}

/* --------------------------------------------------------------------------
   2. Code Block Copy Buttons
   -------------------------------------------------------------------------- */
function initCodeCopyButtons() {
  const codeBlocks = document.querySelectorAll('.markdown-body pre');

  codeBlocks.forEach((pre) => {
    // Avoid double-wrapping
    if (pre.parentElement.classList.contains('code-block-wrapper')) return;

    const wrapper = document.createElement('div');
    wrapper.className = 'code-block-wrapper';
    pre.parentNode.insertBefore(wrapper, pre);
    wrapper.appendChild(pre);

    // Detect language from class (e.g. language-lua, language-rust, language-bash)
    const code = pre.querySelector('code');
    let lang = '';
    if (code) {
      const match = code.className.match(/language-([a-zA-Z0-9_-]+)/);
      if (match) {
        lang = match[1];
      }
    }

    if (lang) {
      const tag = document.createElement('span');
      tag.className = 'code-lang-tag';
      tag.textContent = lang;
      wrapper.appendChild(tag);
    }

    // Create Copy Button
    const copyBtn = document.createElement('button');
    copyBtn.className = 'code-copy-btn';
    copyBtn.setAttribute('aria-label', 'Copy code to clipboard');
    copyBtn.innerHTML = `
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
      </svg>
      <span>Copy</span>
    `;

    copyBtn.addEventListener('click', async () => {
      const rawText = code ? code.innerText : pre.innerText;
      try {
        await navigator.clipboard.writeText(rawText);
        copyBtn.classList.add('copied');
        copyBtn.innerHTML = `
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="20 6 9 17 4 12"></polyline>
          </svg>
          <span>Copied!</span>
        `;
        setTimeout(() => {
          copyBtn.classList.remove('copied');
          copyBtn.innerHTML = `
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
            </svg>
            <span>Copy</span>
          `;
        }, 2000);
      } catch (err) {
        console.error('Failed to copy code to clipboard', err);
      }
    });

    wrapper.appendChild(copyBtn);
  });
}

/* --------------------------------------------------------------------------
   3. In-Page Table of Contents & ScrollSpy
   -------------------------------------------------------------------------- */
function initInPageToc() {
  const article = document.getElementById('articleBody');
  const tocContainer = document.getElementById('inPageToc');
  const tocSection = document.getElementById('inPageTocContainer');

  if (!article || !tocContainer || !tocSection) return;

  const headings = article.querySelectorAll('h2, h3');
  if (headings.length < 2) {
    tocSection.style.display = 'none';
    return;
  }

  tocContainer.innerHTML = '';
  const links = [];

  headings.forEach((h, idx) => {
    if (!h.id) {
      const slug = h.textContent
        .trim()
        .toLowerCase()
        .replace(/[^\w\s-]/g, '')
        .replace(/\s+/g, '-');
      h.id = slug || `heading-${idx}`;
    }

    const a = document.createElement('a');
    a.href = `#${h.id}`;
    a.className = `toc-link ${h.tagName.toLowerCase() === 'h3' ? 'toc-depth-3' : 'toc-depth-2'}`;
    a.textContent = h.textContent.trim();
    a.title = h.textContent.trim();

    tocContainer.appendChild(a);
    links.push({ element: h, link: a });
  });

  // ScrollSpy with IntersectionObserver
  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            const id = entry.target.id;
            links.forEach(({ link }) => {
              if (link.getAttribute('href') === `#${id}`) {
                link.classList.add('active');
              } else {
                link.classList.remove('active');
              }
            });
          }
        });
      },
      {
        rootMargin: '-80px 0px -60% 0px',
        threshold: 0,
      }
    );

    headings.forEach((h) => observer.observe(h));
  }
}

/* --------------------------------------------------------------------------
   4. Quick Search & Keyboard Shortcut
   -------------------------------------------------------------------------- */
function initQuickSearch() {
  const searchInput = document.getElementById('quickSearchInput');
  if (!searchInput) return;

  // Press "/" to focus search bar
  document.addEventListener('keydown', (e) => {
    if (
      e.key === '/' &&
      document.activeElement !== searchInput &&
      !['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName)
    ) {
      e.preventDefault();
      searchInput.focus();
    } else if (e.key === 'Escape' && document.activeElement === searchInput) {
      searchInput.blur();
    }
  });

  // Filter TOC and highlight matching headings
  searchInput.addEventListener('input', (e) => {
    const query = e.target.value.toLowerCase().trim();
    const tocLinks = document.querySelectorAll('.toc-link');

    tocLinks.forEach((link) => {
      const text = link.textContent.toLowerCase();
      if (!query || text.includes(query)) {
        link.style.display = 'block';
      } else {
        link.style.display = 'none';
      }
    });
  });
}

/* --------------------------------------------------------------------------
   5. Mobile Menu Toggle
   -------------------------------------------------------------------------- */
function initMobileMenu() {
  const toggleBtn = document.getElementById('mobileMenuToggle');
  const sidebar = document.getElementById('siteSidebar');
  const backdrop = document.getElementById('sidebarBackdrop');

  if (!toggleBtn || !sidebar || !backdrop) return;

  function toggle() {
    sidebar.classList.toggle('open');
  }

  function close() {
    sidebar.classList.remove('open');
  }

  toggleBtn.addEventListener('click', toggle);
  backdrop.addEventListener('click', close);

  // Close when clicking a link on mobile
  sidebar.querySelectorAll('a').forEach((link) => {
    link.addEventListener('click', close);
  });
}

/* --------------------------------------------------------------------------
   6. Back to Top
   -------------------------------------------------------------------------- */
function initBackToTop() {
  const backBtn = document.getElementById('backToTop');
  if (!backBtn) return;

  backBtn.addEventListener('click', () => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  });
}
