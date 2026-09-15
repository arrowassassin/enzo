import { useEffect, useState } from 'react'
import { Link, NavLink, useLocation } from 'react-router-dom'
import { NAV, REPO } from '../data/site'
import { useTheme } from '../lib/theme'
import { CloseIcon, GithubIcon, MenuIcon, MoonIcon, QuireMark, SunIcon } from './Icons'

export function Nav() {
  const [stuck, setStuck] = useState(false)
  const [open, setOpen] = useState(false)
  const { effective, toggle } = useTheme()
  const location = useLocation()

  useEffect(() => {
    const onScroll = () => setStuck(window.scrollY > 8)
    onScroll()
    window.addEventListener('scroll', onScroll, { passive: true })
    return () => window.removeEventListener('scroll', onScroll)
  }, [])

  useEffect(() => setOpen(false), [location.pathname])

  return (
    <header className="nav" data-stuck={stuck}>
      <div className="nav__inner">
        <Link to="/" className="mark" aria-label="Quire — home">
          <QuireMark className="mark__glyph" />
          <span className="mark__word">Quire</span>
        </Link>

        <nav className="nav__links nav__links--desktop" aria-label="Primary">
          {NAV.map((item) => (
            <NavLink key={item.to} to={item.to} className="nav__link">
              {item.label}
            </NavLink>
          ))}
        </nav>

        <div className="nav__actions">
          <Link className="btn btn--sm nav__cta" to="/downloads">
            Download
          </Link>
          <a
            className="icon-btn nav__ghost"
            href={REPO}
            target="_blank"
            rel="noreferrer"
            aria-label="Quire on GitHub"
          >
            <GithubIcon />
          </a>
          <button
            type="button"
            className="icon-btn"
            onClick={toggle}
            aria-label={effective === 'dark' ? 'Switch to the light theme' : 'Switch to the dark theme'}
          >
            {effective === 'dark' ? <SunIcon /> : <MoonIcon />}
          </button>
          <button
            type="button"
            className="icon-btn nav__toggle"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            aria-controls="nav-drawer"
            aria-label={open ? 'Close the menu' : 'Open the menu'}
          >
            {open ? <CloseIcon /> : <MenuIcon />}
          </button>
        </div>
      </div>

      {open ? (
        <div className="nav__drawer" id="nav-drawer">
          <ul>
            {NAV.map((item) => (
              <li key={item.to}>
                <Link to={item.to}>{item.label}</Link>
              </li>
            ))}
            <li>
              <a href={REPO} target="_blank" rel="noreferrer">
                Source on GitHub
              </a>
            </li>
          </ul>
        </div>
      ) : null}
    </header>
  )
}
