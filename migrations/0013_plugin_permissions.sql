-- Sistema de permisos declarativos para plugins
-- Reemplaza el lexer SQL por validación en tiempo de instalación

-- Tabla de permisos declarativos de plugins
CREATE TABLE IF NOT EXISTS plugin_permissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id UUID NOT NULL REFERENCES plugin_store(id) ON DELETE CASCADE,
    table_name TEXT NOT NULL,
    access_type TEXT NOT NULL CHECK (access_type IN ('read', 'write', 'admin')),
    allowed_columns TEXT[], -- NULL = todas las columnas no sensibles
    requires_user_filter BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    
    -- Un plugin no puede tener permisos duplicados para la misma tabla
    UNIQUE(plugin_id, table_name)
);

-- Índices para performance
CREATE INDEX IF NOT EXISTS idx_plugin_permissions_plugin_id ON plugin_permissions(plugin_id);
CREATE INDEX IF NOT EXISTS idx_plugin_permissions_table_name ON plugin_permissions(table_name);

-- Insertar permisos por defecto para plugins existentes
-- (esto se puede ajustar según las necesidades específicas)
INSERT INTO plugin_permissions (plugin_id, table_name, access_type, allowed_columns, requires_user_filter)
SELECT 
    p.id,
    unnest(ARRAY['heartbeats', 'projects', 'daily_stats_cache']) as table_name,
    'read' as access_type,
    NULL as allowed_columns,
    true as requires_user_filter
FROM plugin_store p
WHERE p.is_published = true 
  AND p.script IS NOT NULL
  AND NOT EXISTS (
    SELECT 1 FROM plugin_permissions pp WHERE pp.plugin_id = p.id
  )
ON CONFLICT (plugin_id, table_name) DO NOTHING;

-- Permisos de lectura para tabla users (columnas seguras)
INSERT INTO plugin_permissions (plugin_id, table_name, access_type, allowed_columns, requires_user_filter)
SELECT 
    p.id,
    'users' as table_name,
    'read' as access_type,
    ARRAY['id', 'username', 'created_at', 'updated_at'] as allowed_columns,
    true as requires_user_filter
FROM plugin_store p
WHERE p.is_published = true 
  AND p.script IS NOT NULL
  AND NOT EXISTS (
    SELECT 1 FROM plugin_permissions pp 
    WHERE pp.plugin_id = p.id AND pp.table_name = 'users'
  )
ON CONFLICT (plugin_id, table_name) DO NOTHING;
