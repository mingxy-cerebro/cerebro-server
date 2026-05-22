export function AppFooter() {
  return (
    <footer className="border-t border-border bg-card px-4 py-2">
      <div className="flex items-center justify-center gap-4 text-xs text-muted-foreground">
        <span>© 2026 omem</span>
        <a
          href="https://beian.miit.gov.cn/"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-foreground transition-colors"
        >
          吉ICP备2026003061号
        </a>
      </div>
    </footer>
  )
}
