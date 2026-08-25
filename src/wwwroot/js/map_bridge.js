// Global Tauri IPC Bridge for Blazor WASM
window.tauriInvoke = async function (cmd, args) {
    if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
        return await window.__TAURI__.core.invoke(cmd, args);
    }
    if (window.__TAURI__ && typeof window.__TAURI__.invoke === 'function') {
        return await window.__TAURI__.invoke(cmd, args);
    }
    if (window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {
        return await window.__TAURI_INTERNALS__.invoke(cmd, args);
    }
    console.error("Tauri IPC not available for command: " + cmd);
    throw new Error("Tauri IPC not available for: " + cmd);
};

// GeoViz Map Bridge: Leaflet Canvas & Vector Manager
window.geoVizMap = {
    map: null,
    baseLayers: {},
    vectorLayers: {},
    measureLayer: null,
    activeMeasureMode: null, // "distance" | "area" | null
    measurePoints: [],
    dotNetRef: null,
    _lastMouseMoveEmit: 0,

    // Normalize snake_case / camelCase style objects into Leaflet options
    resolveStyle: function (style, opacity) {
        const s = style || {};
        const shapeType = (s.shape_type || s.shapeType || 'point').toLowerCase();
        const fillOpacity = s.fill_opacity !== undefined ? s.fill_opacity : (s.fillOpacity !== undefined ? s.fillOpacity : 0.35);
        const finalOpacity = opacity !== undefined ? opacity : 1.0;
        return {
            shapeType: shapeType,
            fillColor: s.fill_color || s.fillColor || '#38bdf8',
            fillOpacity: (shapeType === 'line' ? 0 : fillOpacity) * finalOpacity,
            color: s.stroke_color || s.strokeColor || '#0ea5e9',
            weight: s.stroke_width !== undefined ? s.stroke_width : (s.strokeWidth !== undefined ? s.strokeWidth : 2),
            opacity: (s.stroke_opacity !== undefined ? s.stroke_opacity : (s.strokeOpacity !== undefined ? s.strokeOpacity : 0.9)) * finalOpacity,
            radius: s.point_radius !== undefined ? s.point_radius : (s.pointRadius !== undefined ? s.pointRadius : 6),
            dashArray: s.dash_array || s.dashArray || null,
            fill: shapeType !== 'line',
            // Attribute-driven symbology (backend-computed class breaks)
            classification: s.classification || null,
            labelField: s.label_field || s.labelField || null,
            // Cartographic blending (QGIS parity): composited per layer pane
            blendMode: s.blend_mode || s.blendMode || null
        };
    },

    // The QGIS blending modes, executed by the browser compositor via CSS.
    BLEND_MODES: ['normal', 'multiply', 'screen', 'overlay', 'darken', 'lighten',
        'color-dodge', 'color-burn', 'hard-light', 'soft-light', 'difference',
        'exclusion', 'hue', 'saturation', 'color', 'luminosity'],

    // Dedicated pane per blended layer so mix-blend-mode composites only
    // that layer against everything below it.
    paneForBlend: function (layerId, blendMode) {
        const mode = this.BLEND_MODES.includes(blendMode) && blendMode !== 'normal' ? blendMode : null;
        if (!mode) return undefined;
        const paneName = 'blend-' + layerId;
        let pane = this.map.getPane(paneName);
        if (!pane) {
            pane = this.map.createPane(paneName);
            pane.style.zIndex = 450;
        }
        pane.style.mixBlendMode = mode;
        return paneName;
    },

    // Match a feature's numeric attribute value against class breaks
    classColorFor: function (classification, properties) {
        if (!classification || !properties) return null;
        const raw = properties[classification.field];
        if (raw === undefined || raw === null) return null;
        const value = typeof raw === 'number' ? raw : parseFloat(raw);
        if (!isFinite(value)) return null;
        for (const brk of classification.breaks || []) {
            if (value >= brk.min && value <= brk.max) return brk.color;
        }
        return null;
    },

    initMap: function (containerId, dotNetReference) {
        this.dotNetRef = dotNetReference;
        const savedTheme = localStorage.getItem('geoviz_theme') || 'dark';
        document.documentElement.setAttribute('data-theme', savedTheme);
        const container = document.getElementById(containerId);
        if (!container) {
            console.error("Map container not found: " + containerId);
            return;
        }

        if (this.map) {
            this.map.remove();
            this.map = null;
            this.vectorLayers = {};
        }

        // Initialize Leaflet Map centered globally
        this.map = L.map(containerId, {
            center: [20, 0],
            zoom: 2,
            zoomControl: false,
            attributionControl: false
        });

        L.control.zoom({ position: 'topright' }).addTo(this.map);
        L.control.scale({ position: 'bottomright', imperial: false }).addTo(this.map);

        // Basemaps
        const darkCarto = L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
            maxZoom: 19,
            subdomains: 'abcd'
        });

        const lightCarto = L.tileLayer('https://{s}.basemaps.cartocdn.com/light_all/{z}/{x}/{y}{r}.png', {
            maxZoom: 19,
            subdomains: 'abcd'
        });

        const osm = L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
            maxZoom: 19
        });

        // Esri World Imagery — reliable global satellite tiles (no scraping)
        const satellite = L.tileLayer('https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}', {
            maxZoom: 19,
            attribution: 'Esri, Maxar, Earthstar Geographics'
        });

        const topo = L.tileLayer('https://{s}.tile.opentopomap.org/{z}/{x}/{y}.png', {
            maxZoom: 17
        });

        this.baseLayers = {
            "dark": darkCarto,
            "light": lightCarto,
            "osm": osm,
            "satellite": satellite,
            "topo": topo
        };

        // Basemap: restore the persisted choice (independent of UI theme);
        // first run defaults to the dark carto style.
        let savedBasemap = null;
        try {
            savedBasemap = localStorage.getItem('geoviz_basemap');
        } catch (e) {}
        const activeBasemapKey = savedBasemap && this.baseLayers[savedBasemap] ? savedBasemap : 'dark';
        this.baseLayers[activeBasemapKey].addTo(this.map);
        this.currentBasemap = activeBasemapKey;

        // Mouse Telemetry Listener (throttled: max ~10 updates/sec to avoid IPC/render churn)
        const self = this;
        this.map.on('mousemove', function (e) {
            const now = Date.now();
            if (now - self._lastMouseMoveEmit < 100) return;
            self._lastMouseMoveEmit = now;
            if (self.dotNetRef) {
                self.dotNetRef.invokeMethodAsync('OnMapMouseMove', e.latlng.lat, e.latlng.lng, self.map.getZoom());
            }
        });

        // Initialize Measurement Layer
        this.measureLayer = L.layerGroup().addTo(this.map);

        // Measurement click listener
        this.map.on('click', function (e) {
            if (self.activeMeasureMode) {
                self.handleMeasureClick(e.latlng);
            }
        });

        setTimeout(() => {
            if (self.map) {
                self.map.invalidateSize();
            }
        }, 150);
    },

    setTheme: function (theme) {
        // UI theme ONLY — the map basemap is an independent preference and
        // is never changed as a side effect of toggling the UI theme.
        document.documentElement.setAttribute('data-theme', theme);
        try {
            localStorage.setItem('geoviz_theme', theme);
        } catch (e) {}
    },

    getUiTheme: function () {
        try {
            return localStorage.getItem('geoviz_theme') || 'dark';
        } catch (e) {
            return 'dark';
        }
    },

    switchBasemap: function (basemapKey) {
        if (!this.map || !this.baseLayers[basemapKey]) return;
        Object.values(this.baseLayers).forEach(layer => {
            if (this.map.hasLayer(layer)) {
                this.map.removeLayer(layer);
            }
        });
        this.baseLayers[basemapKey].addTo(this.map);
        this.currentBasemap = basemapKey;
        try {
            localStorage.setItem('geoviz_basemap', basemapKey);
        } catch (e) {}
    },

    // Basemap currently displayed (persisted across sessions).
    getBasemap: function () {
        let saved = null;
        try {
            saved = localStorage.getItem('geoviz_basemap');
        } catch (e) {}
        return this.currentBasemap || (saved && this.baseLayers[saved] ? saved : 'dark');
    },

    addOrUpdateGeoJsonLayer: function (layerId, geoJsonStr, style, isVisible, opacity) {
        if (!this.map) {
            console.warn("Map not initialized yet for layer: " + layerId);
            return;
        }

        // Remove existing layer if present
        if (this.vectorLayers[layerId]) {
            this.map.removeLayer(this.vectorLayers[layerId]);
            delete this.vectorLayers[layerId];
        }

        if (!geoJsonStr) return;

        let geoJsonData;
        try {
            geoJsonData = typeof geoJsonStr === 'string' ? JSON.parse(geoJsonStr) : geoJsonStr;
        } catch (e) {
            console.error("Invalid GeoJSON string for layer " + layerId, e);
            return;
        }

        const self = this;
        const st = this.resolveStyle(style, opacity);
        const blendPane = this.paneForBlend(layerId, st.blendMode);

        const geoLayer = L.geoJSON(geoJsonData, {
            pane: blendPane,
            style: function (feature) {
                const classColor = this.classColorFor(st.classification, feature && feature.properties);
                return {
                    fillColor: classColor || st.fillColor,
                    fillOpacity: st.fillOpacity,
                    color: st.color,
                    weight: st.weight,
                    opacity: st.opacity,
                    dashArray: st.dashArray,
                    fill: st.fill
                };
            }.bind(this),
            pointToLayer: function (feature, latlng) {
                const classColor = this.classColorFor(st.classification, feature.properties);
                return L.circleMarker(latlng, {
                    radius: st.radius,
                    fillColor: classColor || st.fillColor,
                    fillOpacity: st.fillOpacity,
                    color: st.color,
                    weight: st.weight,
                    opacity: st.opacity
                });
            }.bind(this),
            onEachFeature: function (feature, layer) {
                // Attribute label rendering (permanent tooltip)
                if (st.labelField && feature.properties) {
                    const raw = feature.properties[st.labelField];
                    if (raw !== undefined && raw !== null && String(raw).length > 0) {
                        layer.bindTooltip(String(raw), {
                            permanent: true,
                            direction: 'center',
                            className: 'geoviz-feature-label'
                        });
                    }
                }

                // Interactive Popup
                if (feature.properties && Object.keys(feature.properties).length > 0) {
                    let html = '<div class="geoviz-popup-table"><table>';
                    html += '<thead><tr><th>Property</th><th>Value</th></tr></thead><tbody>';
                    for (const [k, v] of Object.entries(feature.properties)) {
                        let displayVal = typeof v === 'object' ? JSON.stringify(v) : String(v ?? '');
                        displayVal = String(displayVal).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
                        html += `<tr><td><strong>${k}</strong></td><td>${displayVal}</td></tr>`;
                    }
                    html += '</tbody></table></div>';
                    layer.bindPopup(html, { maxWidth: 350, className: 'geoviz-custom-popup' });
                }

                layer.on('mouseover', function () {
                    if (!self.activeMeasureMode) {
                        this.setStyle && this.setStyle({ weight: st.weight + 2 });
                    }
                });

                layer.on('mouseout', function () {
                    if (!self.activeMeasureMode) {
                        this.setStyle && this.setStyle({ weight: st.weight });
                    }
                });

                layer.on('click', function (e) {
                    if (self.dotNetRef && !self.activeMeasureMode) {
                        const featureIndex = (geoJsonData.features || []).indexOf(feature);
                        self.dotNetRef.invokeMethodAsync('OnFeatureSelected', layerId, featureIndex, JSON.stringify(feature.properties || {}));
                    }
                });
            }
        });

        this.vectorLayers[layerId] = geoLayer;

        if (isVisible !== false) {
            geoLayer.addTo(this.map);
        }

        this.invalidateSize();
    },

    updateLayerStyle: function (layerId, style, opacity) {
        if (!this.map || !this.vectorLayers[layerId]) return;
        const geoLayer = this.vectorLayers[layerId];
        const st = this.resolveStyle(style, opacity);

        geoLayer.eachLayer(function (l) {
            if (l.setStyle) {
                l.setStyle({
                    fillColor: st.fillColor,
                    fillOpacity: st.fillOpacity,
                    color: st.color,
                    weight: st.weight,
                    opacity: st.opacity,
                    dashArray: st.dashArray,
                    fill: st.fill
                });
            }
            if (l.setRadius) {
                l.setRadius(st.radius);
            }
        });
    },

    setLayerVisibility: function (layerId, isVisible) {
        if (!this.map || !this.vectorLayers[layerId]) return;
        const layer = this.vectorLayers[layerId];
        if (isVisible) {
            if (!this.map.hasLayer(layer)) {
                layer.addTo(this.map);
            }
        } else {
            if (this.map.hasLayer(layer)) {
                this.map.removeLayer(layer);
            }
        }
    },

    removeLayer: function (layerId) {
        if (!this.map || !this.vectorLayers[layerId]) return;
        this.map.removeLayer(this.vectorLayers[layerId]);
        delete this.vectorLayers[layerId];
    },

    zoomToLayer: function (layerId) {
        if (!this.map || !this.vectorLayers[layerId]) return;
        try {
            const bounds = this.vectorLayers[layerId].getBounds();
            if (bounds.isValid()) {
                this.map.fitBounds(bounds, { padding: [50, 50], maxZoom: 16 });
            }
        } catch (e) {
            console.warn("Could not zoom to layer bounds", e);
        }
    },

    // Smart auto-focus after a fresh import: if the layer's on-screen
    // footprint is too small to notice (or lands outside the viewport),
    // fit it into view so users never mistake a loaded layer for a
    // failed one. Returns true when the camera moved.
    focusLayerSmart: function (layerId, minPixelSize) {
        if (!this.map || !this.vectorLayers[layerId]) return false;
        try {
            const layer = this.vectorLayers[layerId];
            if (!layer.getBounds) return false;
            const bounds = layer.getBounds();
            if (!bounds || !bounds.isValid()) return false;

            const size = this.map.getSize();
            if (!size || size.x === 0 || size.y === 0) return false;

            const nw = this.map.latLngToContainerPoint(bounds.getNorthWest());
            const se = this.map.latLngToContainerPoint(bounds.getSouthEast());
            const pxW = Math.abs(se.x - nw.x);
            const pxH = Math.abs(se.y - nw.y);
            const threshold = minPixelSize || 140;

            const offScreen = se.x < 0 || nw.x > size.x || se.y < 0 || nw.y > size.y;
            const tooSmall = pxW < threshold && pxH < threshold;
            const degenerate = !isFinite(pxW) || !isFinite(pxH) || (pxW === 0 && pxH === 0);

            if (tooSmall || offScreen || degenerate) {
                this.map.fitBounds(bounds, { padding: [70, 70], maxZoom: 18 });
                return true;
            }
            return false;
        } catch (e) {
            console.warn("focusLayerSmart failed for " + layerId, e);
            return false;
        }
    },

    fitAllLayers: function () {
        if (!this.map) return;
        const layers = Object.values(this.vectorLayers);
        if (layers.length === 0) return;
        const group = new L.featureGroup(layers);
        try {
            const bounds = group.getBounds();
            if (bounds.isValid()) {
                this.map.fitBounds(bounds, { padding: [50, 50], maxZoom: 16 });
            }
        } catch (e) {
            console.warn("Could not fit all layers", e);
        }
    },

    zoomToFeature: function (lng, lat, zoomLevel) {
        if (!this.map) return;
        this.map.setView([lat, lng], zoomLevel || 12, { animate: true });
        // Pulse marker
        const pulseMarker = L.circleMarker([lat, lng], {
            radius: 14,
            color: '#38bdf8',
            fillColor: '#0284c7',
            fillOpacity: 0.8,
            weight: 3
        }).addTo(this.map);

        setTimeout(() => {
            this.map.removeLayer(pulseMarker);
        }, 2500);
    },

    resetView: function () {
        if (!this.map) return;
        if (Object.keys(this.vectorLayers).length > 0) {
            this.fitAllLayers();
        } else {
            this.map.setView([20, 0], 2, { animate: true });
        }
    },

    // Current camera state (for spatial bookmarks)
    getView: function () {
        if (!this.map) return null;
        const center = this.map.getCenter();
        return { lat: center.lat, lng: center.lng, zoom: this.map.getZoom() };
    },

    setView: function (lat, lng, zoomLevel) {
        if (!this.map) return;
        this.map.setView([lat, lng], zoomLevel, { animate: true });
    },

    setMeasureMode: function (mode) {
        this.activeMeasureMode = mode;
        this.measurePoints = [];
        if (this.measureLayer) {
            this.measureLayer.clearLayers();
        }
        if (this.map) {
            this.map.getContainer().style.cursor = mode ? 'crosshair' : '';
        }
    },

    handleMeasureClick: function (latlng) {
        this.measurePoints.push(latlng);
        const count = this.measurePoints.length;

        // Add vertex marker
        L.circleMarker(latlng, {
            radius: 4,
            color: '#ef4444',
            fillColor: '#ffffff',
            fillOpacity: 1,
            weight: 2
        }).addTo(this.measureLayer);

        if (this.activeMeasureMode === 'distance') {
            if (count > 1) {
                const line = L.polyline(this.measurePoints, { color: '#ef4444', weight: 3, dashArray: '4, 4' });
                this.measureLayer.addLayer(line);

                let totalDist = 0;
                for (let i = 0; i < count - 1; i++) {
                    totalDist += this.measurePoints[i].distanceTo(this.measurePoints[i + 1]);
                }
                const label = totalDist > 1000 ? `${(totalDist / 1000).toFixed(2)} km` : `${totalDist.toFixed(1)} m`;
                L.popup().setLatLng(latlng).setContent(`<strong>Distance:</strong> ${label}`).openOn(this.map);
            }
        } else if (this.activeMeasureMode === 'area') {
            if (count > 2) {
                const poly = L.polygon(this.measurePoints, { color: '#ef4444', fillColor: '#ef4444', fillOpacity: 0.25 });
                this.measureLayer.addLayer(poly);

                const areaSqm = this.calculatePolygonArea(this.measurePoints);
                const label = areaSqm > 1000000 ? `${(areaSqm / 1000000).toFixed(3)} km²` : `${areaSqm.toFixed(1)} m²`;
                L.popup().setLatLng(latlng).setContent(`<strong>Total Area:</strong> ${label}`).openOn(this.map);
            }
        }
    },

    clearMeasurements: function () {
        this.setMeasureMode(null);
        if (this.measureLayer) {
            this.measureLayer.clearLayers();
        }
    },

    calculatePolygonArea: function (latLngs) {
        if (latLngs.length < 3) return 0;
        const R = 6378137;
        let total = 0;
        const n = latLngs.length;
        for (let i = 0; i < n; i++) {
            const p1 = latLngs[i];
            const p2 = latLngs[(i + 1) % n];
            const lambda1 = p1.lng * Math.PI / 180;
            const phi1 = p1.lat * Math.PI / 180;
            const lambda2 = p2.lng * Math.PI / 180;
            const phi2 = p2.lat * Math.PI / 180;
            total += (lambda2 - lambda1) * (2 + Math.sin(phi1) + Math.sin(phi2));
        }
        return Math.abs(total * R * R / 4.0);
    },

    invalidateSize: function () {
        if (this.map) {
            this.map.invalidateSize();
        }
    },

    openFilePicker: function (dotNetHelper) {
        const input = document.createElement('input');
        input.type = 'file';
        input.accept = '.geojson,.json,.shp,.zip,.txt';
        input.style.display = 'none';
        document.body.appendChild(input);

        input.onchange = function (e) {
            const file = e.target.files[0];
            if (file) {
                const ext = file.name.split('.').pop().toLowerCase();
                const isBinary = (ext === 'shp' || ext === 'zip');
                const reader = new FileReader();

                reader.onload = function (evt) {
                    let result = evt.target.result;
                    if (isBinary) {
                        // evt.target.result is a data URL e.g. data:application/zip;base64,AAAA...
                        const commaIdx = result.indexOf(',');
                        if (commaIdx !== -1) {
                            result = result.substring(commaIdx + 1);
                        }
                    }
                    dotNetHelper.invokeMethodAsync('OnJsFileSelected', file.name, file.size, result, isBinary);
                    document.body.removeChild(input);
                };
                reader.onerror = function () {
                    alert('Failed to read selected file.');
                    document.body.removeChild(input);
                };

                if (isBinary) {
                    reader.readAsDataURL(file);
                } else {
                    reader.readAsText(file);
                }
            } else {
                document.body.removeChild(input);
            }
        };

        input.click();
    },

    downloadFile: function (filename, content, mimeType) {
        const blob = new Blob([content], { type: mimeType || 'text/plain;charset=utf-8' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        setTimeout(() => {
            document.body.removeChild(a);
            URL.revokeObjectURL(url);
        }, 100);
    }
};

// Attribute-table helpers: reveal a row inside the virtualized grid.
window.geoVizTable = {
    revealRow: function (index) {
        const el = document.querySelector('tr[data-row-index="' + index + '"]');
        if (el) {
            el.scrollIntoView({ block: 'nearest' });
        }
    }
};
