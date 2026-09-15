import { Route, Routes } from 'react-router-dom'
import { Nav } from './components/Nav'
import { Footer } from './components/Footer'
import { ScrollToTop } from './components/ScrollToTop'
import { Home } from './pages/Home'
import { Features } from './pages/Features'
import { Screens } from './pages/Screens'
import { Guide } from './pages/Guide'
import { Install } from './pages/Install'
import { Downloads } from './pages/Downloads'
import { FaqPage } from './pages/Faq'
import { NotFound } from './pages/NotFound'

export function App() {
  return (
    <div className="shell">
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <ScrollToTop />
      <Nav />
      <main id="main">
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/features" element={<Features />} />
          <Route path="/screens" element={<Screens />} />
          <Route path="/guide" element={<Guide />} />
          <Route path="/install" element={<Install />} />
          <Route path="/downloads" element={<Downloads />} />
          <Route path="/faq" element={<FaqPage />} />
          <Route path="*" element={<NotFound />} />
        </Routes>
      </main>
      <Footer />
    </div>
  )
}
