import { Routes, Route, Navigate } from 'react-router-dom'
import { AppLayout } from '@/components/layout/app-layout'
import { LoginPage } from '@/views/auth/login'
import { DashboardPage } from '@/views/dashboard/dashboard'
import { MemoryListPage } from '@/views/memories/memory-list'
import { MemoryDetailPage } from '@/views/memories/memory-detail'
import { MemoryFormPage } from '@/views/memories/memory-form'
import { MemoryInsightFormPage } from '@/views/memories/memory-insight-form'
import { VaultMemoriesPage } from '@/views/vault/vault-memories'
import { SessionListPage } from '@/views/sessions/session-list'
import { SessionDetailPage } from '@/views/sessions/session-detail'
import { SpacesPage } from '@/views/spaces/spaces'
import { AnalyticsPage } from '@/views/analytics/analytics'
import { TierHistoryPage } from '@/views/tier-history/tier-history'
import { LifecyclePage } from '@/views/lifecycle/lifecycle-page'
import { ImportPage } from '@/views/import/import-page'
import { SettingsPage } from '@/views/settings/settings-page'
import { ProfilePage } from '@/views/profile/profile-page'
import { CategoriesPage } from '@/views/categories/categories-page'
import { NotFoundPage } from '@/views/error/not-found'
import { ErrorBoundary } from '@/components/error-boundary'
import { useAuthStore } from '@/stores/auth'

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const isAuthenticated = useAuthStore((state) => state.isAuthenticated)
  const bypassAuth = typeof window !== 'undefined' && window.localStorage.getItem('e2e_bypass_auth') === 'true'
  return (isAuthenticated || bypassAuth) ? children : <Navigate to="/login" replace />
}

function App() {
  return (
    <ErrorBoundary>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route
          path="/*"
          element={
            <ProtectedRoute>
              <AppLayout />
            </ProtectedRoute>
          }
        >
          <Route index element={<Navigate to="/dashboard" replace />} />
          <Route path="dashboard" element={<DashboardPage />} />
          <Route path="memories" element={<MemoryListPage />} />
          <Route path="memories/:id" element={<MemoryDetailPage />} />
          <Route path="memories/new" element={<MemoryFormPage />} />
          <Route path="memories/:id/edit" element={<MemoryFormPage />} />
          <Route path="memories/:id/edit-insight" element={<MemoryInsightFormPage />} />
          <Route path="vault" element={<VaultMemoriesPage />} />
          <Route path="spaces" element={<SpacesPage />} />
          <Route path="sessions" element={<SessionListPage />} />
          <Route path="sessions/:id" element={<SessionDetailPage />} />
          <Route path="analytics" element={<AnalyticsPage />} />
          <Route path="tier-history" element={<TierHistoryPage />} />
          <Route path="lifecycle" element={<LifecyclePage />} />
          <Route path="import" element={<ImportPage />} />
          <Route path="profile" element={<ProfilePage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="categories" element={<CategoriesPage />} />
          <Route path="*" element={<NotFoundPage />} />
        </Route>
      </Routes>
    </ErrorBoundary>
  )
}

export default App
