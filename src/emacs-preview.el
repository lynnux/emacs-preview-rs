;;; emacs-preview.el ---  preview text file.	-*- lexical-binding: t -*-

;; Copyright (C) 2023 lynnux

;; Author: lynnux <lynnux@qq.com>
;; Version: 0.1.0
;; URL: https://github.com/lynnux/emacs-preview-rs


;; This file is free software: you can redistribute it and/or modify
;; it under the terms of the GNU General Public License as published by
;; the Free Software Foundation, either version 3 of the License, or
;; (at your option) any later version.

;; This file is distributed in the hope that it will be useful,
;; but WITHOUT ANY WARRANTY; without even the implied warranty of
;; MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
;; GNU General Public License for more details.

;; You should have received a copy of the GNU General Public License
;; along with this file.  If not, see <http://www.gnu.org/licenses/>.

;;; Commentary:
;;
;; preview text file.
;;

;;; Code:

(require 'cl-lib)

(defgroup emacs-preview nil
  "Realtime Preview"
  :group 'text
  :prefix "emacs-preview:")

(defcustom emacs-preview:allow-modes '(org-mode markdown-mode html-mode web-mode mhtml-mode)
  "Allow preview modes."
  :type 'list
  :group 'emacs-preview)

(defcustom emacs-preview:host "127.0.0.1"
  "Preview http host."
  :type 'string
  :group 'emacs-preview)

(defcustom emacs-preview:port 8080
  "Preview http port."
  :type 'integer
  :group 'emacs-preview)

(defcustom emacs-preview:browser-open t
  "Auto open browser."
  :type 'boolean
  :group 'emacs-preview)

(defcustom emacs-preview:auto-update t
  "Auto update when preview."
  :type 'boolean
  :group 'emacs-preview)

(defcustom emacs-preview:auto-scroll t
  "Auto scroll when preview."
  :type 'boolean
  :group 'emacs-preview)

(defcustom emacs-preview:text-content '((t . emacs-preview:markdown-content))
  "How to preview text, export to markdown or html."
  :type 'cons
  :group 'emacs-preview)

(defcustom emacs-preview:css-file
  '("/static/css/markdown.css")
  "Custom preview css style."
  :type 'list
  :group 'emacs-preview)

(defcustom emacs-preview:js-file
  '("/static/js/jquery.min.js"
    "/static/js/marked.min.js"
    "/static/js/highlight.min.js"
    "/static/js/mermaid.js")
  "Custom preview js script."
  :type 'list
  :group 'emacs-preview)

(defcustom emacs-preview:check-change-timer-sec 0.5 
  "Timer to check position change of current buffer")

(defcustom emacs-preview:auto-hook nil
  "Hook for user specified auto preview instance.

This hook run within the procedure of `emacs-preview:init' when
customized variable `emacs-preview:auto' was non-nil.

The internal auto-preview type transferred
`emacs-preview:send-to-server' to the `post-self-insert-hook',
this hook providing more customization functional for as."
  :type 'hook
  :group 'emacs-preview)

(defcustom emacs-preview:finialize-hook nil
  "Hooks for run with `emacs-preview:finalize'.
It's useful to remove all dirty hacking with `emacs-preview:auto-hook'."
  :type 'hook
  :group 'emacs-preview)

(defvar emacs-preview:web-index nil)
(defvar emacs-preview:home-path (file-name-directory load-file-name))
(defvar emacs-preview:preview-file (concat emacs-preview:home-path "index.html"))
(defvar emacs-preview:timer nil)
(defvar-local emacs-preview:local-pos nil)

(defun emacs-preview:position-percent ()
  "Preview position percent."
  (when emacs-preview:auto-scroll
    (format
     "<div id=\"position-percentage\" style=\"display:none;\">%s</div>\n"
     (number-to-string
      (truncate (* 100 (/ (float (-  (line-number-at-pos) (/ (count-screen-lines (window-start) (point)) 2)))
                          (count-lines (point-min) (point-max)))))))))

(defun emacs-preview:send-preview()
  (let ((text-content-func (cdr (assoc major-mode emacs-preview:text-content))))
    (setq httpd-root default-directory)
    (unless text-content-func
      (setq text-content-func (cdr (assoc t emacs-preview:text-content))))
    (emacs-preview-rs/web-server-set-root emacs-preview:web-index (vconcat [] (list default-directory emacs-preview:home-path)))
    (emacs-preview-rs/web-server-set-content
     emacs-preview:web-index "/get_content" (concat (emacs-preview:position-percent) (funcall text-content-func)))))

(defun emacs-preview:send-to-server (&optional ws)
  "Send the `emacs-preview' preview to WS clients."
  (when (and emacs-preview:web-index
         (bound-and-true-p emacs-preview-mode)
             (member major-mode emacs-preview:allow-modes))
    (emacs-preview:send-preview)))

(defun emacs-preview:css-template ()
  "Css Template."
  (mapconcat
   (lambda (x)
     (if (string-match-p "^[\n\t ]*<style" x) x
       (format "<link rel=\"stylesheet\" type=\"text/css\" href=\"%s\">" x)))
   emacs-preview:css-file "\n"))

(defun emacs-preview:js-template ()
  "Css Template."
  (mapconcat
   (lambda (x)
     (if (string-match-p "^[\n\t ]*<script" x) x
       (format "<script src=\"%s\"></script>" x)))
   emacs-preview:js-file "\n"))

(defun emacs-preview:preview-template ()
  "Template."
  (with-temp-buffer
    (insert-file-contents emacs-preview:preview-file)
    (when (search-forward "{{ css }}" nil t)
      (replace-match (emacs-preview:css-template) t))
    (when (search-forward "{{ js }}" nil t)
      (replace-match (emacs-preview:js-template) t))
    (when (search-forward "{{ websocket }}" nil t)
      (replace-match (format
                      "%s:%s"
                      emacs-preview:host
                      emacs-preview:websocket-port)
                     t))
    (buffer-string)))

(defun emacs-preview:html-content ()
  "Get file html content."
  (let ((file-name buffer-file-truename))
    (concat (cond ((memq major-mode '(org-mode markdown-mode))
                   (require 'ox-html)
                   (let ((content (org-export-as 'html)))
                     (with-temp-buffer
                       (insert content)
                       (buffer-string))))
                  ((memq major-mode '(web-mode html-mode))
                   (with-temp-buffer
                     (insert-file-contents file-name)
                     (buffer-string)))
                  (t (buffer-substring-no-properties (point-min) (point-max))))
            "<!-- iframe -->")))

(defun emacs-preview:markdown-content ()
  "Get file markdown content."
  (let ((file-name buffer-file-truename))
    (cond ((eq major-mode 'org-mode)
           (require 'ox-md)
           (org-export-as 'md))
          ((memq major-mode '(web-mode html-mode))
           (concat (with-temp-buffer
                     (insert-file-contents file-name)
                     (buffer-string))
                   "<!-- iframe -->"))
          (t (buffer-substring-no-properties (point-min) (point-max))))))

(defun emacs-preview:open-browser ()
  "Open browser."
  (browse-url
   (format "http://%s:%s" emacs-preview:host emacs-preview:port)))

(defun emacs-preview:timer-callback()
  (unless (eq emacs-preview:local-pos (point))
    (setq emacs-preview:local-pos (point))
    (emacs-preview:send-to-server)))

(defun emacs-preview:init ()
  "Preview init."
  (unless emacs-preview:web-index
    (let ((ret (emacs-preview-rs/web-server-start emacs-preview:host emacs-preview:port)))
      (when (> ret 0)
        (setq emacs-preview:web-index ret)
        (emacs-preview-rs/web-server-set-root emacs-preview:web-index (vconcat [] (list emacs-preview:home-path)))
        (emacs-preview-rs/web-server-set-content emacs-preview:web-index "/" (emacs-preview:preview-template)))))
  (when emacs-preview:browser-open (emacs-preview:open-browser))
  (when emacs-preview:auto-update
    (unless emacs-preview:timer
      (setq emacs-preview:timer 
            (run-with-idle-timer emacs-preview:check-change-timer-sec t #'emacs-preview:timer-callback)))
    (add-hook 'post-self-insert-hook #'emacs-preview:send-to-server)
    (run-hooks 'emacs-preview:auto-hook))
  (add-hook 'after-save-hook #'emacs-preview:send-to-server))

(defun emacs-preview:finalize ()
  "Preview close."
  (when emacs-preview:web-index
    (emacs-preview-rs/web-server-stop emacs-preview:web-index)
    (setq emacs-preview:web-index nil))
  (when emacs-preview:timer
    (cancel-timer emacs-preview:timer)
    (setq emacs-preview:timer nil))
  (remove-hook 'post-self-insert-hook 'emacs-preview:send-to-server)
  (remove-hook 'after-save-hook 'emacs-preview:send-to-server))

;;;###autoload
(defun emacs-preview-cleanup ()
  "Cleanup `emacs-preview' mode."
  (interactive)
  (emacs-preview:finalize)
  (run-hooks 'emacs-preview:finialize-hook))

;;;###autoload
(define-minor-mode emacs-preview-mode
  "Emacs preview mode"
  :group      'emacs-preview
  :init-value nil
  :global     t
  (if emacs-preview-mode (emacs-preview:init) (emacs-preview:finalize)))

(provide 'emacs-preview)
;;; emacs-preview.el ends here
