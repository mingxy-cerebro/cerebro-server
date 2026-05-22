import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { Toaster } from 'sonner'
import { ThemeProvider } from '@/providers/theme-provider'
import { TooltipProvider } from '@/components/ui/tooltip'
import { ToastProvider } from '@/providers/toast-provider'
import './index.css'
import App from './App'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter>
      <ThemeProvider defaultTheme="dark" storageKey="omem-theme">
        <TooltipProvider>
          <ToastProvider>
            <App />
          </ToastProvider>
          <Toaster position="top-right" richColors closeButton />
        </TooltipProvider>
      </ThemeProvider>
    </BrowserRouter>
  </StrictMode>,
)
